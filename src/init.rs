use crate::error::Error;

use std::sync::Mutex;

static REF_COUNT: Mutex<usize> = Mutex::new(0);

pub struct InitGuard(());

impl InitGuard {
    pub fn new() -> Result<Self, Error> {
        let mut ref_count = REF_COUNT.lock().unwrap();
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
        *REF_COUNT.lock().unwrap() += 1;

        Self(())
    }
}

impl Drop for InitGuard {
    fn drop(&mut self) {
        let mut ref_count = REF_COUNT.lock().unwrap();
        *ref_count -= 1;

        if *ref_count == 0 {
            unsafe {
                enet_sys::enet_deinitialize();
            }
        }
    }
}
