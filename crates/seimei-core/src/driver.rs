// Copyright 2026 Seimei Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Parameter drivers. The expression is stored as source text; it is parsed
//! and compiled in M1-04.

use crate::elements::ParameterId;

#[derive(Debug, Clone, PartialEq)]
pub struct Driver {
    pub output: ParameterId,
    pub expression: String,
}
