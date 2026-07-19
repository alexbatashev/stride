use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use tokio::sync::oneshot;

type Job = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send>;

pub(crate) struct ExecutionHub {
    tx: mpsc::Sender<Job>,
}

impl ExecutionHub {
    fn new(threads: usize) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let rx = Arc::new(Mutex::new(rx));
        for index in 0..threads {
            let rx = rx.clone();
            std::thread::Builder::new()
                .name(format!("stride-execenv-{index}"))
                .spawn(move || worker_loop(rx))
                .expect("execenv worker thread");
        }
        Self { tx }
    }

    pub(crate) async fn run<T, F>(&self, job: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&tokio::runtime::Runtime) -> T + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Box::new(move |runtime| {
                let _ = tx.send(job(runtime));
            }))
            .map_err(|_| anyhow::anyhow!("execenv execution queue stopped"))?;
        rx.await
            .map_err(|_| anyhow::anyhow!("execenv worker stopped"))
    }
}

pub(crate) fn execution_hub(threads: usize) -> Arc<ExecutionHub> {
    static HUBS: OnceLock<Mutex<HashMap<usize, Arc<ExecutionHub>>>> = OnceLock::new();
    let threads = threads.max(1);
    let hubs = HUBS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut hubs = hubs.lock().expect("execenv hub registry poisoned");
    hubs.entry(threads)
        .or_insert_with(|| Arc::new(ExecutionHub::new(threads)))
        .clone()
}

fn worker_loop(rx: Arc<Mutex<mpsc::Receiver<Job>>>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("execenv worker runtime");
    loop {
        let job = rx.lock().expect("execenv execution queue poisoned").recv();
        let Ok(job) = job else {
            break;
        };
        job(&runtime);
    }
}
