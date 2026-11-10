//! Thread creation, scheduling, waiting and notifying.

use super::*;

/// Resumes allowed in one tick before the rest are deferred to the next one (a `wait 0` loop).
const MAX_RESUMES_PER_TICK: u64 = 2_000_000;

impl<H: Host> Vm<H> {
    pub(super) fn run_tick(&mut self) {
        if let Some(due) = self.timeline.remove(&self.tick) {
            let mut front: VecDeque<ThreadId> = VecDeque::new();
            for id in due {
                if let Some(t) = self.threads.get_mut(&id)
                    && matches!(t.state, State::Sleeping(w) if w == self.tick)
                {
                    t.state = State::Ready;
                    front.push_back(id);
                }
            }
            // Timers were queued before anything notified since: they run first.
            front.append(&mut self.ready);
            self.ready = front;
        }
        self.drain_ready();
        self.tick += 1;
    }

    pub(super) fn drain_ready(&mut self) {
        let mut resumes: u64 = 0;
        loop {
            while let Some(id) = self.ready.pop_front() {
                resumes += 1;
                if resumes > MAX_RESUMES_PER_TICK {
                    // A thread that never yields time: push it to the next tick.
                    if let Some(t) = self.threads.get_mut(&id) {
                        t.state = State::Sleeping(self.tick + 1);
                        self.timeline.entry(self.tick + 1).or_default().push(id);
                    }
                    if resumes == MAX_RESUMES_PER_TICK + 1 {
                        self.errors.push(ScriptError {
                            thread: id,
                            time_ms: self.time_ms(),
                            function: String::new(),
                            offset: 0,
                            message: "too many thread resumes in one tick (wait 0 loop?); deferring".into(),
                            stack: Vec::new(),
                        });
                    }
                    continue;
                }
                self.resume(id);
            }
            if self.frame_end.is_empty() {
                break;
            }
            while let Some(id) = self.frame_end.pop_front() {
                if let Some(t) = self.threads.get_mut(&id) {
                    t.state = State::Ready;
                    self.ready.push_back(id);
                }
            }
        }
    }

    fn resume(&mut self, id: ThreadId) {
        let Some(mut th) = self.threads.remove(&id) else { return };
        if !matches!(th.state, State::Ready) {
            self.threads.insert(id, th);
            return;
        }
        match std::mem::replace(&mut th.wake, Wake::None) {
            Wake::None => {}
            Wake::Push(v) => th.stack.push(v),
            Wake::Args(args) => {
                th.stack.push(Value::FrameBase);
                for a in args.into_iter().rev() {
                    th.stack.push(a);
                }
            }
        }
        self.run_and_settle(th);
    }

    /// Creates a thread for `func` and runs it at once (nested under the running thread when
    /// `extra_nest` is the caller's frame count).
    pub(super) fn spawn(
        &mut self,
        scripts: &Arc<ScriptSet>,
        func: FuncRef,
        this: Value,
        args: Vec<Value>,
        parent_uid: Option<u64>,
        extra_nest: usize,
    ) -> ThreadId {
        let id = self.next_thread;
        self.next_thread += 1;
        let mut th = Thread {
            id,
            frames: Vec::new(),
            stack: vec![Value::CallMarker],
            obj_reg: Value::Undefined,
            var_reg: VarRef::None,
            wake: Wake::None,
            state: State::Ready,
            regs: Vec::new(),
            wait_desc: None,
        };
        for a in args.into_iter().rev() {
            th.stack.push(a);
        }
        let uid = self.next_uid;
        self.next_uid += 1;
        let first = scripts
            .get(func.script)
            .and_then(|p| p.script.functions.get(func.func as usize))
            .map(|f| f.first);
        let Some(first) = first else {
            self.errors.push(ScriptError {
                thread: id,
                time_ms: self.time_ms(),
                function: scripts.describe(func),
                offset: 0,
                message: "no such function".into(),
                stack: Vec::new(),
            });
            return id;
        };
        th.frames.push(Frame { func, ip: first, locals: Vec::new(), base: 0, self_val: this, uid });
        if let Some(p) = parent_uid {
            self.children.entry(p).or_default().push(id);
        }
        if self.tracing() {
            let target = scripts.describe(func);
            self.emit(TraceEvent::ThreadStart { thread: id, target, child: parent_uid.is_some() });
        }
        if let Some(c) = self.census.as_mut() {
            c.threads_started += 1;
            *c.calls.entry((func.script, func.func)).or_insert(0) += 1;
        }
        self.nest += extra_nest;
        self.run_and_settle(th);
        self.nest -= extra_nest;
        id
    }

    pub(super) fn run_and_settle(&mut self, mut th: Thread) {
        let scripts = self.scripts.clone();
        let id = th.id;
        self.running.push(id);
        let mut budget = self.budget;
        let out = self.run(&scripts, &mut th, &mut budget);
        self.running.pop();
        let killed = self.kill_flags.remove(&id);
        match out {
            _ if killed => self.cleanup(th, true),
            Ok(Flow::Done(v)) => {
                self.ret_slot = Some((id, v));
                self.cleanup(th, false);
            }
            Ok(Flow::Blocked) => self.park(th),
            Ok(Flow::Killed) => self.cleanup(th, true),
            Err(VmErr(message)) => {
                let err = self.make_error(&scripts, &th, message);
                if self.tracing() {
                    self.emit(TraceEvent::Error(err.to_string()));
                }
                if let Some(c) = self.census.as_mut() {
                    c.errors += 1;
                }
                self.errors.push(err);
                self.cleanup(th, true);
            }
        }
    }

    fn make_error(&self, scripts: &ScriptSet, th: &Thread, message: String) -> ScriptError {
        let (function, offset) = match th.frames.last() {
            Some(f) => {
                let off = scripts
                    .get(f.func.script)
                    .and_then(|p| p.script.instrs.get(f.ip.saturating_sub(1)))
                    .map(|i| i.offset)
                    .unwrap_or(0);
                (scripts.describe(f.func), off)
            }
            None => (String::new(), 0),
        };
        let stack = th.frames.iter().rev().map(|f| scripts.describe(f.func)).collect();
        ScriptError { thread: th.id, time_ms: self.time_ms(), function, offset, message, stack }
    }

    fn park(&mut self, th: Thread) {
        let id = th.id;
        let state = th.state.clone();
        self.threads.insert(id, th);
        match state {
            State::Sleeping(t) => {
                if t <= self.tick {
                    if let Some(t) = self.threads.get_mut(&id) {
                        t.state = State::Ready;
                    }
                    self.ready.push_back(id);
                } else {
                    self.timeline.entry(t).or_default().push(id);
                }
            }
            State::FrameEnd => self.frame_end.push_back(id),
            State::Ready => self.ready.push_back(id),
            State::Notify => {}
        }
    }

    /// Removes a finished or killed thread's registrations and kills its child threads.
    fn cleanup(&mut self, th: Thread, killed: bool) {
        if self.tracing() {
            self.emit(TraceEvent::ThreadEnd { thread: th.id, killed });
        }
        for key in &th.regs {
            if let Some(list) = self.waiters.get_mut(key) {
                list.retain(|w| w.thread != th.id);
                if list.is_empty() {
                    self.waiters.remove(key);
                }
            }
        }
        for f in &th.frames {
            if let Some(ch) = self.children.remove(&f.uid) {
                for c in ch {
                    self.kill_thread(c);
                }
            }
        }
    }

    pub(super) fn kill_thread(&mut self, id: ThreadId) {
        if let Some(th) = self.threads.remove(&id) {
            self.cleanup(th, true);
        } else if self.running.contains(&id) {
            self.kill_flags.insert(id);
        }
    }

    pub(super) fn register_wait(&mut self, th: &mut Thread, key: WKey, kind: WaitKind) {
        let seq = self.next_seq;
        self.next_seq += 1;
        if !th.regs.contains(&key) {
            th.regs.push(key.clone());
        }
        self.waiters.entry(key).or_default().push(Waiter { thread: th.id, seq, kind });
    }

    fn remove_waiter(&mut self, key: &WKey, seq: u64) {
        if let Some(list) = self.waiters.get_mut(key) {
            list.retain(|w| w.seq != seq);
            if list.is_empty() {
                self.waiters.remove(key);
            }
        }
    }

    fn wake(&mut self, id: ThreadId, wake: Wake) {
        if let Some(t) = self.threads.get_mut(&id) {
            t.wake = wake;
            t.state = State::Ready;
            t.wait_desc = None;
            self.ready.push_back(id);
        }
    }

    pub(super) fn do_notify(&mut self, target: &Value, name: &str, args: Vec<Value>) {
        let Some(obj) = obj_key(target) else { return };
        if let Some(c) = self.census.as_mut() {
            *c.notifies.entry(name.to_ascii_lowercase()).or_insert(0) += 1;
        }
        if self.tracing() {
            self.emit(TraceEvent::Notify { target: describe_value(target), name: name.to_owned(), args: args.len() });
        }
        let key = WKey { obj, name: name.to_ascii_lowercase() };
        let Some(snapshot) = self.waiters.get(&key).cloned() else { return };
        for w in snapshot {
            let present = self.waiters.get(&key).is_some_and(|l| l.iter().any(|x| x.seq == w.seq));
            if !present {
                continue;
            }
            match &w.kind {
                WaitKind::Endon => {
                    self.remove_waiter(&key, w.seq);
                    self.kill_thread(w.thread);
                }
                WaitKind::Waittill { matches } => {
                    if !args_match(matches, &args) {
                        continue;
                    }
                    self.remove_waiter(&key, w.seq);
                    self.wake(w.thread, Wake::Args(args.clone()));
                }
                WaitKind::Host => {
                    self.remove_waiter(&key, w.seq);
                    self.wake(w.thread, Wake::Push(Value::Undefined));
                }
            }
        }
    }
}

/// `waittillmatch`: the notify's leading arguments must equal the match values.
fn args_match(matches: &[Value], args: &[Value]) -> bool {
    if args.len() < matches.len() {
        return false;
    }
    matches.iter().zip(args).all(|(m, a)| ops::equal(m, a).unwrap_or(false))
}
