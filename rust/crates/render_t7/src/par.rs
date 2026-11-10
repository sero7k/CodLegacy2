//! Work done side by side on a few threads: pipeline creation, picture
//! decoding and uploads are independent of one another and the drivers
//! take them from several threads, so the load spreads them over the
//! machine's cores (`BO2MP_T7_THREADS=n` sets the count; 1 is serial).

/// How many worker threads a parallel step uses.
pub fn workers() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("BO2MP_T7_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n >= 1)
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(2, usize::from).saturating_sub(1).clamp(2, 12))
    })
}

/// `f` over `items` on up to [`workers`] threads; the results in the items'
/// order. (Items are taken one at a time, so a few slow ones do not hold up
/// the rest.)
pub fn map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = workers().min(items.len());
    if n <= 1 {
        return items.iter().map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Vec<std::sync::Mutex<Option<R>>> = items.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = f(item);
                    if let Ok(mut slot) = out[i].lock() {
                        *slot = Some(r);
                    }
                }
            });
        }
    });
    out.into_iter().map(|m| m.into_inner().ok().flatten().expect("a worker finished without a result")).collect()
}
