//! [`LazyText`]: a document's full text, built at most once and only if a
//! server actually needs it (full sync, mismatched encoding, didSave text).

use std::sync::{Arc, Mutex, OnceLock};

type MakeText = Box<dyn FnOnce() -> String + Send>;

/// Full document text that is produced on first use and then shared.
pub(crate) struct LazyText {
    text: OnceLock<Arc<str>>,
    make: Mutex<Option<MakeText>>,
}

impl LazyText {
    /// Text that is already available.
    pub(crate) fn ready(text: Arc<str>) -> Arc<Self> {
        Arc::new(LazyText {
            text: OnceLock::from(text),
            make: Mutex::new(None),
        })
    }

    /// Text produced by `make` the first time [`LazyText::get`] is called.
    pub(crate) fn deferred(make: impl FnOnce() -> String + Send + 'static) -> Arc<Self> {
        Arc::new(LazyText {
            text: OnceLock::new(),
            make: Mutex::new(Some(Box::new(make))),
        })
    }

    /// The text, building it now if needed. Concurrent callers wait for the
    /// first one; the producer runs at most once and is then dropped.
    pub(crate) fn get(&self) -> Arc<str> {
        self.text
            .get_or_init(|| {
                let make = self.make.lock().unwrap_or_else(|e| e.into_inner()).take();
                make.map(|f| Arc::from(f()))
                    .unwrap_or_else(|| Arc::from(""))
            })
            .clone()
    }

    /// True once the text has been built (or was given up front).
    #[cfg(test)]
    pub(crate) fn is_built(&self) -> bool {
        self.text.get().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn deferred_text_is_built_once_on_demand() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let text = LazyText::deferred(move || {
            c.fetch_add(1, Ordering::SeqCst);
            "hello".to_string()
        });
        assert!(!text.is_built());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let t = text.clone();
                std::thread::spawn(move || t.get())
            })
            .collect();
        for t in threads {
            assert_eq!(&*t.join().unwrap(), "hello");
        }
        assert_eq!(&*text.get(), "hello");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(text.is_built());
    }

    #[test]
    fn ready_text_needs_no_producer() {
        let text = LazyText::ready(Arc::from("x"));
        assert!(text.is_built());
        assert_eq!(&*text.get(), "x");
    }

    #[test]
    fn unused_producer_is_never_called() {
        let text = LazyText::deferred(|| panic!("must not run"));
        drop(text);
    }
}
