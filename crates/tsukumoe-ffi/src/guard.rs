// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The common wrapper of every `extern "C"` function: a panic never crosses
//! the boundary, and the reason of a failure is kept per thread.

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

use tsukumoe_core::tmod;
use tsukumoe_runtime::Error;

/// The result of every call.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsukumoeStatus {
    Ok = 0,
    /// A required pointer argument is NULL.
    NullArgument = 1,
    /// The `.tmod` is broken, or a string is not UTF-8.
    InvalidData = 2,
    /// An index, layer, size, enumeration value or number is out of range.
    OutOfRange = 3,
    /// No parameter, motion or expression has that name.
    NotFound = 4,
    /// A panic was caught inside the library.
    Internal = 5,
}

/// A failure: the status and the message `tsukumoe_last_error_message` returns.
pub(crate) struct Failure(pub TsukumoeStatus, pub String);

pub(crate) type Res = Result<(), Failure>;

pub(crate) fn fail(status: TsukumoeStatus, message: impl Into<String>) -> Failure {
    Failure(status, message.into())
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        let status = match e {
            Error::Load(_) | Error::Layout(_) => TsukumoeStatus::InvalidData,
            Error::UnknownMotion(_) | Error::UnknownExpression(_) | Error::UnknownParameter(_) => {
                TsukumoeStatus::NotFound
            }
            Error::BadLayer(_) | Error::BadAutoValue(_) => TsukumoeStatus::OutOfRange,
        };
        Failure(status, e.to_string())
    }
}

impl From<tmod::Error> for Failure {
    fn from(e: tmod::Error) -> Self {
        Error::from(e).into()
    }
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_last_error(message: &str) {
    // An interior NUL would cut the string; the message is for people.
    let c = CString::new(message.replace('\0', " ")).unwrap_or_default();
    let _ = LAST_ERROR.try_with(|e| *e.borrow_mut() = c);
}

/// Runs `f`, turning an error or a panic into a status and a message.
pub(crate) fn guard(f: impl FnOnce() -> Res) -> TsukumoeStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => {
            set_last_error("");
            TsukumoeStatus::Ok
        }
        Ok(Err(Failure(status, message))) => {
            set_last_error(&message);
            status
        }
        Err(payload) => {
            let what = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            set_last_error(&format!("internal error: {what}"));
            TsukumoeStatus::Internal
        }
    }
}

/// The message of the last failed call on this thread: UTF-8, NUL-terminated,
/// empty after a successful call. Valid until the next call into the library
/// on the same thread. Never NULL.
#[unsafe(no_mangle)]
pub extern "C" fn tsukumoe_last_error_message() -> *const c_char {
    LAST_ERROR
        .try_with(|e| e.borrow().as_ptr())
        .unwrap_or(c"".as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn message() -> String {
        unsafe { CStr::from_ptr(tsukumoe_last_error_message()) }
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn a_panic_becomes_internal_and_later_calls_still_work() {
        assert_eq!(guard(|| panic!("boom")), TsukumoeStatus::Internal);
        assert_eq!(message(), "internal error: boom");
        assert_eq!(
            guard(|| panic!("{}", String::from("formatted"))),
            TsukumoeStatus::Internal
        );
        assert_eq!(message(), "internal error: formatted");
        assert_eq!(
            guard(|| Err(fail(TsukumoeStatus::NotFound, "no such thing"))),
            TsukumoeStatus::NotFound
        );
        assert_eq!(message(), "no such thing");
        assert_eq!(guard(|| Ok(())), TsukumoeStatus::Ok);
        assert_eq!(message(), "");
    }

    #[test]
    fn the_message_is_per_thread() {
        guard(|| Err(fail(TsukumoeStatus::NotFound, "main")));
        std::thread::spawn(|| assert_eq!(message(), ""))
            .join()
            .unwrap();
        assert_eq!(message(), "main");
    }
}
