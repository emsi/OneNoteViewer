use std::panic::{self, AssertUnwindSafe};
use std::sync::{mpsc, OnceLock};

type TestResult = std::thread::Result<()>;
type TestCommand = (fn(), mpsc::Sender<TestResult>);

static GTK_TESTS: OnceLock<mpsc::Sender<TestCommand>> = OnceLock::new();

pub(crate) fn run_gtk_test(test: fn()) {
    let tests = GTK_TESTS.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<TestCommand>();
        std::thread::spawn(move || {
            let available = gtk::init().is_ok();
            while let Ok((test, result)) = receiver.recv() {
                let outcome = if available {
                    panic::catch_unwind(AssertUnwindSafe(test))
                } else {
                    Ok(())
                };
                let _ignored = result.send(outcome);
            }
        });
        sender
    });
    let (result, receiver) = mpsc::channel();
    tests.send((test, result)).expect("GTK test worker");
    if let Err(panic) = receiver.recv().expect("GTK test result") {
        panic::resume_unwind(panic);
    }
}
