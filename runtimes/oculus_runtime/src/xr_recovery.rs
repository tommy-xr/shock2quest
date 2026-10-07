//! Retry bookkeeping independent of Android, tested with injected runtime failures.

pub struct PendingFrame<T>(Option<T>);

impl<T> Default for PendingFrame<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> PendingFrame<T> {
    pub fn begin<E>(
        &mut self,
        wait: impl FnOnce() -> Result<T, E>,
        begin: impl FnOnce() -> Result<(), E>,
    ) -> Result<T, E> {
        if self.0.is_none() {
            self.0 = Some(wait()?);
        }
        begin()?;
        Ok(self.0.take().expect("successful wait owns a pending frame"))
    }

    pub fn reset(&mut self) {
        self.0 = None;
    }
}

pub trait SwapchainImages {
    type Error;
    fn acquire(&mut self) -> Result<u32, Self::Error>;
    fn wait(&mut self) -> Result<(), Self::Error>;
    fn release(&mut self) -> Result<(), Self::Error>;
}

#[derive(Default)]
pub struct ImageLease {
    acquired: Option<u32>,
    waited: bool,
}

impl ImageLease {
    pub fn acquire<S: SwapchainImages>(&mut self, images: &mut S) -> Result<u32, S::Error> {
        // A failed release leaves the wrapper's waited flag set. Finish it
        // before either acquiring or waiting again.
        if self.waited {
            self.release(images)?;
        }
        let index = match self.acquired {
            Some(index) => index,
            None => {
                let index = images.acquire()?;
                self.acquired = Some(index);
                index
            }
        };
        images.wait()?;
        self.waited = true;
        Ok(index)
    }

    pub fn release<S: SwapchainImages>(&mut self, images: &mut S) -> Result<(), S::Error> {
        images.release()?;
        self.waited = false;
        self.acquired = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn failed_begin_retries_without_waiting_for_another_frame() {
        let mut pending = PendingFrame::default();
        let waits = Cell::new(0);
        let wait = || {
            waits.set(waits.get() + 1);
            Ok(42)
        };
        assert_eq!(pending.begin(wait, || Err("begin")), Err("begin"));
        assert_eq!(pending.begin(wait, || Ok::<_, &str>(())), Ok(42));
        assert_eq!(
            waits.get(),
            1,
            "a second wait deadlocks before a successful begin"
        );
        pending.reset();
        assert_eq!(pending.begin(wait, || Ok::<_, &str>(())), Ok(42));
        assert_eq!(waits.get(), 2);
    }

    #[test]
    fn failed_wait_does_not_begin_and_session_reset_discards_pending_frame() {
        let mut pending = PendingFrame::default();
        assert_eq!(
            pending.begin(|| Err::<u32, _>("wait"), || panic!("must not begin")),
            Err("wait")
        );
        assert_eq!(pending.begin(|| Ok(1), || Err("begin")), Err("begin"));
        pending.reset();
        assert_eq!(pending.begin(|| Ok(2), || Ok::<_, &str>(())), Ok(2));
    }

    #[derive(Default)]
    struct Images {
        acquired: bool,
        waited: bool,
        fail: Option<&'static str>,
        calls: Vec<&'static str>,
    }
    impl Images {
        fn call(&mut self, name: &'static str) -> Result<(), &'static str> {
            self.calls.push(name);
            if self.fail == Some(name) {
                self.fail = None;
                Err(name)
            } else {
                Ok(())
            }
        }
    }
    impl SwapchainImages for Images {
        type Error = &'static str;
        fn acquire(&mut self) -> Result<u32, Self::Error> {
            assert!(
                !self.acquired,
                "acquired a second image while the first is pending"
            );
            self.call("acquire")?;
            self.acquired = true;
            Ok(3)
        }
        fn wait(&mut self) -> Result<(), Self::Error> {
            assert!(self.acquired && !self.waited);
            self.call("wait")?;
            self.waited = true;
            Ok(())
        }
        fn release(&mut self) -> Result<(), Self::Error> {
            assert!(self.waited, "released an image before a successful wait");
            self.call("release")?;
            self.acquired = false;
            self.waited = false;
            Ok(())
        }
    }

    #[test]
    fn failed_acquire_can_retry() {
        let mut lease = ImageLease::default();
        let mut images = Images {
            fail: Some("acquire"),
            ..Default::default()
        };
        assert_eq!(lease.acquire(&mut images), Err("acquire"));
        assert_eq!(lease.acquire(&mut images), Ok(3));
        lease.release(&mut images).unwrap();
        assert_eq!(images.calls, ["acquire", "acquire", "wait", "release"]);
    }

    #[test]
    fn failed_wait_keeps_the_acquired_image() {
        let mut lease = ImageLease::default();
        let mut images = Images {
            fail: Some("wait"),
            ..Default::default()
        };
        assert_eq!(lease.acquire(&mut images), Err("wait"));
        assert_eq!(lease.acquire(&mut images), Ok(3));
        lease.release(&mut images).unwrap();
        assert_eq!(images.calls, ["acquire", "wait", "wait", "release"]);
    }

    #[test]
    fn failed_release_retries_release_before_acquiring_or_waiting() {
        let mut lease = ImageLease::default();
        let mut images = Images {
            fail: Some("release"),
            ..Default::default()
        };
        assert_eq!(lease.acquire(&mut images), Ok(3));
        assert_eq!(lease.release(&mut images), Err("release"));
        assert_eq!(lease.acquire(&mut images), Ok(3));
        lease.release(&mut images).unwrap();
        assert_eq!(
            images.calls,
            [
                "acquire", "wait", "release", "release", "acquire", "wait", "release"
            ]
        );
    }
}
