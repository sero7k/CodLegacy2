//! The interpreter loop.

use super::*;

impl Thread {
    fn pop(&mut self) -> VR<Value> {
        self.stack.pop().ok_or_else(|| VmErr("operand stack underflow".to_owned()))
    }
    fn fr(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("a running thread has a frame")
    }
    fn local_index(&mut self, depth: usize) -> VR<usize> {
        let n = self.fr().locals.len();
        if depth >= n {
            return Err(VmErr(format!("local at depth {depth} does not exist ({n} locals)")));
        }
        Ok(n - 1 - depth)
    }
}

fn arg_u8(ins: &gsc_iw6::Instr) -> usize {
    match ins.operands.first() {
        Some(Operand::U8(n)) => usize::from(*n),
        _ => 0,
    }
}

fn token(ins: &gsc_iw6::Instr) -> VR<&Name> {
    match ins.operands.first() {
        Some(Operand::Token(n)) => Ok(n),
        _ => Err("missing field name".into()),
    }
}

fn jump_target(ins: &gsc_iw6::Instr) -> VR<usize> {
    match ins.operands.first() {
        Some(Operand::Jump(t)) if *t != usize::MAX => Ok(*t),
        _ => Err("bad jump target".into()),
    }
}

/// The type name used in messages about the `self`/object of a field access.
fn is_object(v: &Value) -> bool {
    matches!(v, Value::Object(_) | Value::Entity(_))
}

impl<H: Host> Vm<H> {
    // ---- fields and variables -----------------------------------------------------------

    fn read_field(&mut self, obj: &Value, name: &Name) -> VR<Value> {
        match obj {
            Value::Entity(e) => {
                if let Some(c) = self.census.as_mut() {
                    *c.entity_field_reads.entry(name.to_string()).or_insert(0) += 1;
                }
                if let Some(v) = self.host.get_entity_field(*e, name) {
                    return Ok(v);
                }
                Ok(self.ent_vars.get(&e.0).and_then(|o| o.get(name)).unwrap_or(Value::Undefined))
            }
            Value::Object(o) => {
                if let Some(c) = self.census.as_mut()
                    && o.kind() != ObjKind::Struct
                {
                    *c.level_field_reads.entry(name.to_string()).or_insert(0) += 1;
                }
                Ok(o.get(name).unwrap_or(Value::Undefined))
            }
            other => Err(VmErr(format!("cannot read field {name} of {}", other.type_name()))),
        }
    }

    fn write_field(&mut self, obj: &Value, name: &Name, v: Value) -> VR<()> {
        match obj {
            Value::Entity(e) => {
                if let Some(c) = self.census.as_mut() {
                    *c.entity_field_writes.entry(name.to_string()).or_insert(0) += 1;
                }
                if self.host.set_entity_field(*e, name, &v) {
                    return Ok(());
                }
                if v.is_undefined() {
                    if let Some(o) = self.ent_vars.get(&e.0) {
                        o.remove(name);
                    }
                } else {
                    if !self.ent_vars.contains_key(&e.0) {
                        let o = self.new_struct();
                        self.ent_vars.insert(e.0, o);
                    }
                    if let Some(o) = self.ent_vars.get(&e.0) {
                        o.set(name.clone(), v);
                    }
                }
                Ok(())
            }
            Value::Object(o) => {
                if let Some(c) = self.census.as_mut()
                    && o.kind() != ObjKind::Struct
                {
                    *c.level_field_writes.entry(name.to_string()).or_insert(0) += 1;
                }
                if v.is_undefined() {
                    o.remove(name);
                } else {
                    o.set(name.clone(), v);
                }
                Ok(())
            }
            other => Err(VmErr(format!("cannot set field {name} of {}", other.type_name()))),
        }
    }

    fn read_var(&mut self, th: &mut Thread, r: &VarRef) -> VR<Value> {
        match r {
            VarRef::None => Err("no variable selected".into()),
            VarRef::Local(i) => th.fr().locals.get(*i).cloned().ok_or_else(|| VmErr("bad local".to_owned())),
            VarRef::Game => Ok(self.game.clone()),
            VarRef::Field(o, n) => self.read_field(o, n),
            VarRef::Elem(a, k) => Ok(a.get(k).unwrap_or(Value::Undefined)),
        }
    }

    fn write_var(&mut self, th: &mut Thread, r: &VarRef, v: Value) -> VR<()> {
        match r {
            VarRef::None => Err("no variable selected".into()),
            VarRef::Local(i) => {
                *th.fr().locals.get_mut(*i).ok_or_else(|| VmErr("bad local".to_owned()))? = v;
                Ok(())
            }
            VarRef::Game => {
                self.game = v;
                Ok(())
            }
            VarRef::Field(o, n) => self.write_field(o, n, v),
            VarRef::Elem(a, k) => {
                if v.is_undefined() {
                    a.remove(k);
                } else {
                    a.set(k.clone(), v);
                }
                Ok(())
            }
        }
    }

    /// `variable register = (base)[key]`; an undefined base becomes a new array.
    fn ref_array(&mut self, th: &mut Thread, base: VarRef, key: Value) -> VR<()> {
        let key = Key::from_value(&key)
            .ok_or_else(|| VmErr(format!("{} is not a valid array index", key.type_name())))?;
        let cur = self.read_var(th, &base)?;
        let arr = match cur {
            Value::Array(a) => a,
            Value::Undefined => {
                let a = ArrRef::new();
                self.write_var(th, &base, Value::Array(a.clone()))?;
                a
            }
            other => return Err(VmErr(format!("cannot index {} as an array", other.type_name()))),
        };
        th.var_reg = VarRef::Elem(arr, key);
        Ok(())
    }

    fn array_get(container: &Value, key: &Value) -> VR<Value> {
        match container {
            Value::Array(a) => {
                let k = Key::from_value(key)
                    .ok_or_else(|| VmErr(format!("{} is not a valid array index", key.type_name())))?;
                Ok(a.get(&k).unwrap_or(Value::Undefined))
            }
            Value::Str(s) | Value::IStr(s) => match key {
                Value::Int(i) if *i >= 0 && (*i as usize) < s.len() => {
                    let b = s.as_bytes()[*i as usize];
                    Ok(Value::Str(Arc::from(String::from_utf8_lossy(&[b]).as_ref())))
                }
                _ => Err("string index out of range".into()),
            },
            Value::Vector(v) => match key {
                Value::Int(i) if (0..3).contains(i) => Ok(Value::Float(v[*i as usize])),
                _ => Err("vector index out of range".into()),
            },
            other => Err(VmErr(format!("cannot index {}", other.type_name()))),
        }
    }

    fn marker_index(th: &Thread) -> VR<usize> {
        th.stack
            .iter()
            .rposition(|v| matches!(v, Value::CallMarker))
            .ok_or_else(|| VmErr("call without a call marker".to_owned()))
    }

    fn push_frame(
        &mut self,
        scripts: &ScriptSet,
        th: &mut Thread,
        func: FuncRef,
        self_val: Value,
        base: usize,
    ) -> VR<()> {
        if self.nest + th.frames.len() >= MAX_DEPTH {
            return Err("script stack overflow (too many embedded function calls)".into());
        }
        let first = scripts
            .get(func.script)
            .and_then(|p| p.script.functions.get(func.func as usize))
            .map(|f| f.first)
            .ok_or_else(|| VmErr("call to a missing function".to_owned()))?;
        let uid = self.next_uid;
        self.next_uid += 1;
        if let Some(c) = self.census.as_mut() {
            *c.calls.entry((func.script, func.func)).or_insert(0) += 1;
        }
        if self.tracing() {
            self.emit(TraceEvent::Call { thread: th.id, depth: th.frames.len(), target: scripts.describe(func) });
        }
        th.frames.push(Frame { func, ip: first, locals: Vec::new(), base, self_val, uid });
        Ok(())
    }

    fn far_target(scripts: &ScriptSet, ins: &gsc_iw6::Instr) -> VR<FuncRef> {
        match ins.operands.first() {
            Some(Operand::Far { file, func }) => scripts
                .resolve(file, func)
                .ok_or_else(|| VmErr(format!("unresolved function {file}::{func}"))),
            _ => Err("missing call target".into()),
        }
    }

    fn absent_agent_call(scripts: &ScriptSet, ins: &gsc_iw6::Instr) -> bool {
        match ins.operands.first() {
            Some(Operand::Far { file, func }) => {
                scripts.resolve(file, func).is_none() && file.to_string().starts_with("maps/mp/agents/_agent_utility")
            }
            _ => false,
        }
    }

    fn local_target(prep: &Prepared, script: u32, ins: &gsc_iw6::Instr) -> VR<FuncRef> {
        match ins.operands.first() {
            Some(Operand::Func(off)) => prep
                .func_at(*off)
                .map(|func| FuncRef { script, func })
                .ok_or_else(|| VmErr("call target is not a function".to_owned())),
            _ => Err("missing call target".into()),
        }
    }

    /// Returns from the top frame; `Some(flow)` when the thread ended.
    fn do_return(&mut self, th: &mut Thread, v: Value) -> Option<Flow> {
        let f = th.frames.pop().expect("a frame to return from");
        if let Some(ch) = self.children.remove(&f.uid) {
            for c in ch {
                self.kill_thread(c);
            }
        }
        if th.frames.is_empty() {
            th.stack.clear();
            return Some(Flow::Done(v));
        }
        th.stack.truncate(f.base);
        th.stack.push(v);
        None
    }

    fn start_nested(
        &mut self,
        scripts: &Arc<ScriptSet>,
        th: &mut Thread,
        func: FuncRef,
        this: Value,
        argc: usize,
        child: bool,
    ) -> VR<()> {
        if self.nest + th.frames.len() >= MAX_DEPTH {
            return Err("script stack overflow (too many embedded function calls)".into());
        }
        if !is_object(&this) {
            return Err(VmErr(format!("cannot thread on {}", this.type_name())));
        }
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(th.pop()?);
        }
        let parent = if child { Some(th.fr().uid) } else { None };
        let n = th.frames.len();
        self.spawn(scripts, func, this, args, parent, n);
        th.stack.push(Value::Undefined);
        Ok(())
    }

    fn apply_actions(&mut self, scripts: &Arc<ScriptSet>, th: &Thread, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Notify { target, name, args } => self.do_notify(&target, &name, args),
                Action::StartThread { func, this, args } => {
                    let n = th.frames.len();
                    self.spawn(scripts, func, this, args, None, n);
                }
            }
        }
    }

    fn do_builtin(
        &mut self,
        scripts: &Arc<ScriptSet>,
        th: &mut Thread,
        id: u16,
        argc: usize,
        method: bool,
    ) -> VR<Option<Flow>> {
        let this = if method { Some(th.pop()?) } else { None };
        let mut args = Vec::with_capacity(argc);
        for _ in 0..argc {
            args.push(th.pop()?);
        }
        if let Some(c) = self.census.as_mut() {
            if method {
                *c.builtin_methods.entry(id).or_insert(0) += 1;
            } else {
                *c.builtin_functions.entry(id).or_insert(0) += 1;
            }
        }
        if self.tracing() {
            self.emit(TraceEvent::Builtin { thread: th.id, id, method, argc });
        }
        if !method {
            let ids = self.core_ids;
            if Some(id) == ids.isdefined {
                th.stack.push(Value::Int(i32::from(!args.first().is_none_or(Value::is_undefined))));
                return Ok(None);
            }
            if Some(id) == ids.spawnstruct {
                th.stack.push(Value::Object(ObjRef::new_struct()));
                return Ok(None);
            }
            if Some(id) == ids.getarraykeys {
                let r = match args.first() {
                    Some(Value::Array(a)) => Value::array(a.keys().iter().map(Key::to_value)),
                    other => {
                        return Err(VmErr(format!(
                            "getarraykeys needs an array, not {}",
                            other.map_or("nothing", Value::type_name)
                        )))
                    }
                };
                th.stack.push(r);
                return Ok(None);
            }
            if Some(id) == ids.getfirstarraykey {
                let r = match args.first() {
                    Some(Value::Array(a)) => a.first_key().map(|k| k.to_value()).unwrap_or(Value::Undefined),
                    _ => return Err("getfirstarraykey needs an array".into()),
                };
                th.stack.push(r);
                return Ok(None);
            }
            if Some(id) == ids.getnextarraykey {
                let r = match (args.first(), args.get(1)) {
                    (Some(Value::Array(a)), Some(k)) => {
                        let k = Key::from_value(k).ok_or_else(|| VmErr("bad array key".to_owned()))?;
                        a.next_key(&k).map(|k| k.to_value()).unwrap_or(Value::Undefined)
                    }
                    _ => return Err("getnextarraykey needs an array and a key".into()),
                };
                th.stack.push(r);
                return Ok(None);
            }
        }
        if let Some(t) = &this
            && t.is_undefined()
        {
            return Err("builtin method called on undefined".into());
        }
        let level = Value::Object(self.level.clone());
        let self_val = th.fr().self_val.clone();
        let mut ctx = CallCtx {
            thread: th.id,
            self_val: &self_val,
            level: &level,
            time_ms: self.tick * u64::from(TICK_MS),
            actions: Vec::new(),
        };
        let r = match &this {
            Some(t) => self.host.call_method(id, &mut ctx, t, &args),
            None => self.host.call_function(id, &mut ctx, &args),
        };
        let actions = std::mem::take(&mut ctx.actions);
        let r = r.map_err(VmErr)?;
        self.apply_actions(scripts, th, actions);
        match r {
            HostReturn::Value(v) => th.stack.push(v),
            HostReturn::None => th.stack.push(Value::Undefined),
            HostReturn::Unimplemented => {
                *self.unimplemented.entry((method, id)).or_insert(0) += 1;
                if self.strict {
                    return Err(VmErr(format!(
                        "unimplemented builtin {} {id:#06x}",
                        if method { "method" } else { "function" }
                    )));
                }
                th.stack.push(Value::Undefined);
            }
            HostReturn::Wait(secs) => {
                let ticks = ticks_for(&Value::Float(secs))?;
                th.wake = Wake::Push(Value::Undefined);
                th.state = State::Sleeping(self.tick + ticks);
                return Ok(Some(Flow::Blocked));
            }
            HostReturn::WaitNotify { target, name } => {
                let obj = obj_key(&target).ok_or_else(|| VmErr("cannot wait on a non-object".to_owned()))?;
                th.state = State::Notify;
                th.wait_desc = Some((describe_value(&target), name.clone()));
                self.register_wait(th, WKey { obj, name: name.to_ascii_lowercase() }, WaitKind::Host);
                return Ok(Some(Flow::Blocked));
            }
        }
        Ok(None)
    }

    fn name_arg(v: &Value) -> VR<String> {
        v.as_str().map(str::to_owned).ok_or_else(|| VmErr(format!("notify name must be a string, not {}", v.type_name())))
    }

    /// Runs `th` until it blocks, ends or fails.
    pub(super) fn run(&mut self, scripts: &Arc<ScriptSet>, th: &mut Thread, budget: &mut u64) -> VR<Flow> {
        macro_rules! bin {
            ($f:path) => {{
                let b = th.pop()?;
                let a = th.pop()?;
                th.stack.push($f(&a, &b)?);
            }};
        }
        macro_rules! cmp {
            ($f:path) => {{
                let b = th.pop()?;
                let a = th.pop()?;
                th.stack.push(Value::Int(i32::from($f(&a, &b)?)));
            }};
        }
        loop {
            if *budget == 0 {
                return Err("instruction budget exhausted (runaway loop?)".into());
            }
            *budget -= 1;
            if !self.kill_flags.is_empty() && self.kill_flags.contains(&th.id) {
                return Ok(Flow::Killed);
            }
            let (script_idx, ip) = {
                let f = th.fr();
                (f.func.script, f.ip)
            };
            let prep: &Prepared = scripts.get(script_idx).ok_or("script vanished")?;
            let Some(ins) = prep.script.instrs.get(ip) else {
                return Err("ran past the end of the script".into());
            };
            th.fr().ip = ip + 1;
            if self.trace_ops {
                eprintln!("{:#06x} {} stack={:?} locals={:?}", ins.offset, ins.mnemonic, th.stack, th.frames.last().map(|f| &f.locals));
            }
            if let Some(c) = self.census.as_mut() {
                c.instructions += 1;
            }
            match ins.op {
                0x17 => {
                    let v = th.pop()?;
                    th.fr().locals.push(v);
                }
                0x18 => {
                    let s = th.fr().self_val.clone();
                    th.obj_reg = s.clone();
                    let v = self.read_field(&s, token(ins)?)?;
                    th.stack.push(v);
                }
                0x19 | 0x34 => {
                    let v = if ins.op == 0x19 { th.pop()? } else { Value::Undefined };
                    if let Some(f) = self.do_return(th, v) {
                        return Ok(f);
                    }
                }
                0x1a..=0x1f => {
                    let Some(Operand::Builtin(id)) = ins.operands.first() else { return Err("bad builtin".into()) };
                    if let Some(f) = self.do_builtin(scripts, th, *id, usize::from(ins.op - 0x1a), false)? {
                        return Ok(f);
                    }
                }
                0x20 => {
                    let (Some(Operand::Argc(n)), Some(Operand::Builtin(id))) = (ins.operands.first(), ins.operands.get(1)) else {
                        return Err("bad builtin".into());
                    };
                    if let Some(f) = self.do_builtin(scripts, th, *id, usize::from(*n), false)? {
                        return Ok(f);
                    }
                }
                0x21 => {
                    let v = th.pop()?;
                    th.stack.push(Value::Int(i32::from(!ops::cond(&v)?)));
                }
                0x22 | 0x2f | 0x3e | 0x42 | 0x43 | 0x6b | 0x6c | 0x7f => {
                    let child = matches!(ins.op, 0x2f | 0x3e | 0x6c | 0x7f);
                    let method = matches!(ins.op, 0x22 | 0x3e | 0x6b | 0x6c);
                    let local = matches!(ins.op, 0x43 | 0x6b | 0x6c | 0x7f);
                    let func = if local { Self::local_target(prep, script_idx, ins)? } else { Self::far_target(scripts, ins)? };
                    let Some(Operand::Argc(n)) = ins.operands.get(1) else { return Err("missing argc".into()) };
                    let this = if method { th.pop()? } else { th.fr().self_val.clone() };
                    self.start_nested(scripts, th, func, this, usize::from(*n), child)?;
                }
                0x3a | 0x66 | 0x77 | 0xa0 => {
                    let child = matches!(ins.op, 0x66 | 0xa0);
                    let method = matches!(ins.op, 0x3a | 0x66);
                    let fv = th.pop()?;
                    let Value::Function(func) = fv else {
                        return Err(VmErr(format!("cannot thread {}", fv.type_name())));
                    };
                    let this = if method { th.pop()? } else { th.fr().self_val.clone() };
                    self.start_nested(scripts, th, func, this, arg_u8(ins), child)?;
                }
                0x23 | 0x4b => {
                    let v = th.pop()?;
                    let b = ops::cond(&v)?;
                    let jump = if ins.op == 0x23 { b } else { !b };
                    if jump {
                        th.stack.push(Value::Int(i32::from(b)));
                        let idx = prep.idx(jump_target(ins)?).ok_or("bad jump")?;
                        th.fr().ip = idx;
                    }
                }
                0x24 => {
                    let v = th.pop()?;
                    let l = self.level();
                    self.write_field(&l, token(ins)?, v)?;
                }
                0x25 => {
                    let v = th.pop()?;
                    th.stack.push(Value::Int(i32::from(ops::cond(&v)?)));
                }
                0x26 | 0x5e | 0x5f => {
                    if ins.op == 0x26 {
                        th.fr().locals.push(Value::Undefined);
                    }
                    let depth = if ins.op == 0x5f { arg_u8(ins) } else { 0 };
                    let key = th.pop()?;
                    let idx = th.local_index(depth)?;
                    self.ref_array(th, VarRef::Local(idx), key)?;
                }
                0x27 => {
                    let fv = th.pop()?;
                    let Value::BuiltinFn(id) = fv else {
                        return Err(VmErr(format!("cannot call {} as a builtin function", fv.type_name())));
                    };
                    if let Some(f) = self.do_builtin(scripts, th, id, arg_u8(ins), false)? {
                        return Ok(f);
                    }
                }
                0x9a => {
                    let fv = th.pop()?;
                    let Value::BuiltinMethod(id) = fv else {
                        return Err(VmErr(format!("cannot call {} as a builtin method", fv.type_name())));
                    };
                    if let Some(f) = self.do_builtin(scripts, th, id, arg_u8(ins), true)? {
                        return Ok(f);
                    }
                }
                0x28 => cmp!(neq),
                0x29 => th.stack.push(Value::Int(th.id as i32)),
                0x2a => {
                    let o = th.obj_reg.clone();
                    self.write_field(&o, token(ins)?, Value::Undefined)?;
                }
                0x2b => {
                    let Some(Operand::F32(f)) = ins.operands.first() else { return Err("bad float".into()) };
                    th.stack.push(Value::Float(*f));
                }
                0x2c => {
                    let v = if matches!(th.stack.last(), Some(Value::CallMarker)) { Value::Undefined } else { th.pop()? };
                    th.fr().locals.push(v);
                }
                0x64 | 0x65 => {
                    if !matches!(th.stack.last(), Some(Value::CallMarker)) {
                        let v = th.pop()?;
                        let depth = if ins.op == 0x65 { arg_u8(ins) } else { 0 };
                        let idx = th.local_index(depth)?;
                        th.fr().locals[idx] = v;
                    }
                }
                0x2d | 0x5c => {
                    th.stack.push(Value::CallMarker);
                    let base = th.stack.len() - 1;
                    let func = if ins.op == 0x2d { Self::far_target(scripts, ins)? } else { Self::local_target(prep, script_idx, ins)? };
                    let s = th.fr().self_val.clone();
                    self.push_frame(scripts, th, func, s, base)?;
                }
                0x2e | 0x5d => {
                    let base = Self::marker_index(th)?;
                    if ins.op == 0x2e && Self::absent_agent_call(scripts, ins) {
                        // bo2mp: the agent utility script is in no zone we have; a map with no
                        // bots has no agents, so "all agents" answers an empty array.
                        th.stack.truncate(base);
                        th.stack.push(Value::Array(ArrRef::new()));
                        continue;
                    }
                    let func = if ins.op == 0x2e { Self::far_target(scripts, ins)? } else { Self::local_target(prep, script_idx, ins)? };
                    let s = th.fr().self_val.clone();
                    self.push_frame(scripts, th, func, s, base)?;
                }
                0x50 | 0x9d => {
                    let obj = th.pop()?;
                    if !is_object(&obj) {
                        return Err(VmErr(format!("cannot call a method on {}", obj.type_name())));
                    }
                    let base = Self::marker_index(th)?;
                    let func = if ins.op == 0x9d { Self::far_target(scripts, ins)? } else { Self::local_target(prep, script_idx, ins)? };
                    self.push_frame(scripts, th, func, obj, base)?;
                }
                0x54 | 0x48 => {
                    let fv = th.pop()?;
                    let Value::Function(func) = fv else {
                        return Err(VmErr(format!("cannot call {}", fv.type_name())));
                    };
                    let s = if ins.op == 0x48 {
                        let obj = th.pop()?;
                        if !is_object(&obj) {
                            return Err(VmErr(format!("cannot call a method on {}", obj.type_name())));
                        }
                        obj
                    } else {
                        th.fr().self_val.clone()
                    };
                    let base = Self::marker_index(th)?;
                    self.push_frame(scripts, th, func, s, base)?;
                }
                0x30 | 0x31 => {
                    let depth = if ins.op == 0x31 { arg_u8(ins) } else { 0 };
                    let idx = th.local_index(depth)?;
                    th.fr().locals[idx] = Value::Undefined;
                }
                0x32 => match th.stack.last_mut() {
                    Some(m @ Value::CallMarker) => *m = Value::FrameBase,
                    _ => return Err("function called with too many parameters".into()),
                },
                0x33 | 0x87 => {
                    let v = if ins.op == 0x33 {
                        th.pop()?
                    } else {
                        let idx = th.local_index(arg_u8(ins))?;
                        th.fr().locals[idx].clone()
                    };
                    if !is_object(&v) {
                        return Err(VmErr(format!("{} is not an object", v.type_name())));
                    }
                    th.obj_reg = v;
                }
                0x35 => {
                    let v = th.pop()?;
                    th.stack.push(Value::Int(match &v {
                        Value::Array(a) => a.len() as i32,
                        Value::Str(s) | Value::IStr(s) => s.len() as i32,
                        Value::Object(_) | Value::Entity(_) => 1,
                        other => return Err(VmErr(format!("{} has no size", other.type_name()))),
                    }));
                }
                0x36 => th.stack.push(Value::Array(ArrRef::new())),
                0x37 => bin!(ops::bit_and),
                0x38 => cmp!(ops::less_equal),
                0x39 | 0x88 => th.stack.push(Value::CallMarker),
                0x3b => {}
                0x3d => bin!(ops::div),
                0x3f | 0xa6 => {
                    let Some(Operand::U16(n)) = ins.operands.first() else { return Err("bad operand".into()) };
                    th.stack.push(Value::Int(if ins.op == 0x3f { i32::from(*n) } else { -i32::from(*n) }));
                }
                0x40 | 0xaf => {
                    let v = th.pop()?;
                    let b = ops::cond(&v)?;
                    if b == (ins.op == 0x40) {
                        let idx = prep.idx(jump_target(ins)?).ok_or("bad jump")?;
                        th.fr().ip = idx;
                    }
                }
                0x41 => {
                    let s = th.fr().self_val.clone();
                    th.stack.push(s);
                }
                0x44 | 0x45 => {
                    let v = th.pop()?;
                    let depth = if ins.op == 0x45 { arg_u8(ins) } else { 0 };
                    let idx = th.local_index(depth)?;
                    th.fr().locals[idx] = v;
                }
                0x46 => bin!(ops::add),
                0x47 => {
                    let v = th.pop()?;
                    th.stack.push(ops::bit_not(&v)?);
                }
                0x49 | 0x86 => {
                    let r = th.var_reg.clone();
                    let v = self.read_var(th, &r)?;
                    let Value::Int(i) = v else {
                        return Err(VmErr(format!("cannot {} {}", if ins.op == 0x49 { "increment" } else { "decrement" }, v.type_name())));
                    };
                    let n = if ins.op == 0x49 { i.wrapping_add(1) } else { i.wrapping_sub(1) };
                    // (`inc`/`dec` leave the new value; the `set_variable_field` after them stores it.)
                    th.stack.push(Value::Int(n));
                }
                0x4a => {
                    for _ in 0..arg_u8(ins) {
                        th.fr().locals.pop();
                    }
                }
                0x4c => {
                    let v = th.pop()?;
                    let Some(Operand::Table(toff)) = ins.operands.first() else { return Err("bad switch".into()) };
                    let tidx = prep.idx(*toff).ok_or("bad switch table")?;
                    let Some(Operand::Cases(cases)) = prep.script.instrs[tidx].operands.first() else {
                        return Err("bad switch table".into());
                    };
                    let mut target = None;
                    match &v {
                        Value::Int(_) | Value::Str(_) => {}
                        other => return Err(VmErr(format!("cannot switch on {}", other.type_name()))),
                    }
                    for c in cases {
                        let hit = match (&c.value, &v) {
                            (gsc_iw6::CaseValue::Int(a), Value::Int(b)) => a == b,
                            (gsc_iw6::CaseValue::Str(a), Value::Str(b)) => a.eq_ignore_ascii_case(b),
                            _ => false,
                        };
                        if hit {
                            target = Some(c.target);
                            break;
                        }
                    }
                    if target.is_none()
                        && let Some(last) = cases.last()
                        && last.value == gsc_iw6::CaseValue::Default
                    {
                        target = Some(last.target);
                    }
                    th.fr().ip = match target {
                        Some(t) => prep.idx(t).ok_or("bad case target")?,
                        None => tidx + 1,
                    };
                }
                0x4d => {
                    while !matches!(th.stack.last(), Some(Value::FrameBase) | None) {
                        th.stack.pop();
                    }
                }
                0x4e | 0x4f => {
                    let depth = if ins.op == 0x4f { arg_u8(ins) } else { 0 };
                    let idx = th.local_index(depth)?;
                    th.var_reg = VarRef::Local(idx);
                }
                0x51 => {
                    let o = th.obj_reg.clone();
                    let v = self.read_field(&o, token(ins)?)?;
                    th.stack.push(v);
                }
                0x52 => {
                    th.var_reg = VarRef::Field(th.obj_reg.clone(), token(ins)?.clone());
                }
                0x53 | 0x95 => {
                    let v = prep.konst(ip).cloned().ok_or("string constant missing")?;
                    if let Some(c) = self.census.as_mut()
                        && let Some(s) = v.as_str()
                    {
                        *c.strings.entry(s.to_owned()).or_insert(0) += 1;
                    }
                    th.stack.push(v);
                }
                0x55 => {
                    let l = self.level();
                    th.obj_reg = l.clone();
                    let v = self.read_field(&l, token(ins)?)?;
                    th.stack.push(v);
                }
                0x56 => {
                    let Some(Operand::Vec3(v)) = ins.operands.first() else { return Err("bad vector".into()) };
                    th.stack.push(Value::Vector(*v));
                }
                0x57 | 0x85 | 0xa4 => {
                    let obj = th.pop()?;
                    let name = Self::name_arg(&th.pop()?)?;
                    let key = WKey {
                        obj: obj_key(&obj).ok_or_else(|| VmErr(format!("cannot wait on {}", obj.type_name())))?,
                        name: name.to_ascii_lowercase(),
                    };
                    match ins.op {
                        0x57 => {
                            if let Some(c) = self.census.as_mut() {
                                *c.endons.entry(key.name.clone()).or_insert(0) += 1;
                            }
                            self.register_wait(th, key, WaitKind::Endon);
                        }
                        _ => {
                            let mut matches = Vec::new();
                            if ins.op == 0xa4 {
                                let Some(Operand::U16(n)) = ins.operands.first() else { return Err("bad operand".into()) };
                                for _ in 0..(*n & 0xff) {
                                    matches.push(th.pop()?);
                                }
                                matches.reverse();
                            }
                            if let Some(c) = self.census.as_mut() {
                                *c.waittills.entry(key.name.clone()).or_insert(0) += 1;
                            }
                            if self.tracing() {
                                self.emit(TraceEvent::Waittill { thread: th.id, target: describe_value(&obj), name: key.name.clone() });
                            }
                            th.state = State::Notify;
                            th.wait_desc = Some((describe_value(&obj), key.name.clone()));
                            self.register_wait(th, key, WaitKind::Waittill { matches });
                            return Ok(Flow::Blocked);
                        }
                    }
                }
                0x58 => cmp!(ops::greater_equal),
                0x59 => {
                    th.obj_reg = th.fr().self_val.clone();
                }
                0x5a => {
                    let v = th.pop()?;
                    let a = Value::Object(self.anim.clone());
                    self.write_field(&a, token(ins)?, v)?;
                }
                0x5b => {
                    let v = th.pop()?;
                    let r = th.var_reg.clone();
                    self.write_var(th, &r, v)?;
                }
                0x60 | 0xa5 => {
                    let f = if ins.op == 0x60 { Self::far_target(scripts, ins)? } else { Self::local_target(prep, script_idx, ins)? };
                    th.stack.push(Value::Function(f));
                }
                0x61 => cmp!(ops::less),
                0x62 => th.var_reg = VarRef::Game,
                0x63 => {
                    th.state = State::FrameEnd;
                    return Ok(Flow::Blocked);
                }
                0x67 => th.stack.push(self.level()),
                0x68 => {
                    let obj = th.pop()?;
                    let name = Self::name_arg(&th.pop()?)?;
                    let mut args = Vec::new();
                    loop {
                        match th.stack.pop() {
                            Some(Value::CallMarker) => break,
                            Some(v) => args.push(v),
                            None => return Err("notify without a call marker".into()),
                        }
                    }
                    if obj_key(&obj).is_none() {
                        return Err(VmErr(format!("cannot notify {}", obj.type_name())));
                    }
                    self.host.on_notify(&obj, &name, &args);
                    self.do_notify(&obj, &name, args);
                }
                0x69 => {
                    th.pop()?;
                }
                0x6a => bin!(ops::shl),
                0x6d => cmp!(ops::greater),
                0x6e..=0x73 => {
                    let idx = th.local_index(usize::from(ins.op - 0x6e))?;
                    let v = th.fr().locals[idx].clone();
                    th.stack.push(v);
                }
                0x74 => {
                    let idx = th.local_index(arg_u8(ins))?;
                    let v = th.fr().locals[idx].clone();
                    th.stack.push(v);
                }
                0x75 => {
                    let idx = th.local_index(arg_u8(ins))?;
                    let v = if matches!(th.stack.last(), Some(Value::FrameBase) | None) { Value::Undefined } else { th.pop()? };
                    th.fr().locals[idx] = v;
                }
                0x76 | 0x91 => {
                    let idx = prep.idx(jump_target(ins)?).ok_or("bad jump")?;
                    th.fr().ip = idx;
                }
                0x78 => th.stack.push(Value::Int(0)),
                0x79 => {
                    let v = th.pop()?;
                    let ticks = ticks_for(&v)?;
                    if let Some(c) = self.census.as_mut() {
                        c.waits += 1;
                    }
                    if self.tracing() {
                        self.emit(TraceEvent::Wait { thread: th.id, ticks });
                    }
                    th.state = State::Sleeping(self.tick + ticks);
                    return Ok(Flow::Blocked);
                }
                0x7a => bin!(ops::sub),
                0x7b => {
                    let v = th.pop()?;
                    let s = th.fr().self_val.clone();
                    self.write_field(&s, token(ins)?, v)?;
                }
                0x7d => bin!(ops::mul),
                0x7e => th.fr().locals.push(Value::Undefined),
                0x80 => {
                    let Some(Operand::I32(n)) = ins.operands.first() else { return Err("bad int".into()) };
                    th.stack.push(Value::Int(*n));
                }
                0x81 => bin!(ops::rem),
                0x82 => {
                    let a = Value::Object(self.anim.clone());
                    th.obj_reg = a.clone();
                    th.var_reg = VarRef::Field(a, token(ins)?.clone());
                }
                0x83 | 0x99 => {
                    let Some(Operand::Builtin(id)) = ins.operands.first() else { return Err("bad builtin".into()) };
                    th.stack.push(if ins.op == 0x83 { Value::BuiltinFn(*id) } else { Value::BuiltinMethod(*id) });
                }
                0x84 => th.stack.push(self.game.clone()),
                0x89 => th.stack.push(Value::Object(self.anim.clone())),
                0x8a => th.stack.push(Value::Undefined),
                0x8b => {
                    let l = self.level();
                    th.obj_reg = l.clone();
                    th.var_reg = VarRef::Field(l, token(ins)?.clone());
                }
                0x8c => th.obj_reg = Value::Object(self.anim.clone()),
                0x8d => th.obj_reg = self.level(),
                0x8e => bin!(ops::bit_xor),
                0x8f => cmp!(ops::equal),
                0x90 => {
                    let key = th.pop()?;
                    let r = th.var_reg.clone();
                    match self.read_var(th, &r)? {
                        Value::Array(a) => {
                            let k = Key::from_value(&key).ok_or_else(|| VmErr("bad array index".to_owned()))?;
                            a.remove(&k);
                        }
                        Value::Undefined => {}
                        other => return Err(VmErr(format!("cannot remove an element of {}", other.type_name()))),
                    }
                }
                0x92 => {
                    let Some(Operand::Anim { tree, name }) = ins.operands.first() else { return Err("bad animation".into()) };
                    th.stack.push(Value::Anim(Arc::new(AnimRef { tree: tree.clone(), name: name.clone() })));
                }
                0x93 => {
                    let a = Value::Object(self.anim.clone());
                    th.obj_reg = a.clone();
                    let v = self.read_field(&a, token(ins)?)?;
                    th.stack.push(v);
                }
                0x94 | 0x9f | 0x98 => {
                    let Some(Operand::U8(n)) = ins.operands.first() else { return Err("bad operand".into()) };
                    th.stack.push(Value::Int(if ins.op == 0x98 { -i32::from(*n) } else { i32::from(*n) }));
                }
                0x96 => {
                    let key = th.pop()?;
                    let base = th.var_reg.clone();
                    self.ref_array(th, base, key)?;
                }
                0x97 => {
                    let s = th.fr().self_val.clone();
                    th.obj_reg = s.clone();
                    th.var_reg = VarRef::Field(s, token(ins)?.clone());
                }
                0x9b => {
                    let container = th.pop()?;
                    let key = th.pop()?;
                    th.stack.push(Self::array_get(&container, &key)?);
                }
                0x9c => {
                    let x = th.pop()?;
                    let y = th.pop()?;
                    let z = th.pop()?;
                    let mut c = [0f32; 3];
                    for (slot, v) in c.iter_mut().zip([&x, &y, &z]) {
                        *slot = v.as_float().ok_or_else(|| VmErr(format!("vector component is {}", v.type_name())))?;
                    }
                    th.stack.push(Value::Vector(c));
                }
                0x9e => {
                    let key = th.pop()?;
                    let idx = th.local_index(arg_u8(ins))?;
                    let c = th.fr().locals[idx].clone();
                    th.stack.push(Self::array_get(&c, &key)?);
                }
                0xa1 => bin!(ops::bit_or),
                0xa2 => {
                    let v = th.pop()?;
                    match th.stack.last() {
                        Some(Value::Array(a)) => a.push(v),
                        _ => return Err("array literal without an array".into()),
                    }
                }
                0xa7 => bin!(ops::shr),
                0xa8..=0xad => {
                    let Some(Operand::Builtin(id)) = ins.operands.first() else { return Err("bad builtin".into()) };
                    if let Some(f) = self.do_builtin(scripts, th, *id, usize::from(ins.op - 0xa8), true)? {
                        return Ok(f);
                    }
                }
                0xae => {
                    let (Some(Operand::Argc(n)), Some(Operand::Builtin(id))) = (ins.operands.first(), ins.operands.get(1)) else {
                        return Err("bad builtin".into());
                    };
                    if let Some(f) = self.do_builtin(scripts, th, *id, usize::from(*n), true)? {
                        return Ok(f);
                    }
                }
                op => return Err(VmErr(format!("unimplemented opcode {op:#04x}"))),
            }
        }
    }
}

fn neq(a: &Value, b: &Value) -> Result<bool, String> {
    ops::equal(a, b).map(|e| !e)
}
