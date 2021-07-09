use crate::error::Error;

use parking_lot::Mutex;

static REF_COUNT: Mutex<usize> = parking_lot::const_mutex(0);

pub struct InitGuard(());

impl InitGuard {
    pub fn new() -> Result<Self, Error> {
        let mut ref_count = REF_COUNT.lock();
        if *ref_count != 0 {
            *ref_count += 1;
            return Ok(Self(()));
        }

        // Not initialized.
        if *ref_count == 0 {
            let ret = unsafe { enet_sys::enet_initialize() };
            if ret < 0 {
                return Err(Error::Init);
            }

            *ref_count = 1;
        }

        Ok(Self(()))
    }
}

impl Clone for InitGuard {
    fn clone(&self) -> Self {
        *REF_COUNT.lock() += 1;

        Self(())
    }
}

impl Drop for InitGuard {
    fn drop(&mut self) {
        let mut ref_count = REF_COUNT.lock();
        *ref_count -= 1;

        if *ref_count == 0 {
            unsafe {
                enet_sys::enet_deinitialize();
            }
        }
    }
}
