// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Parameter drivers. The expression is stored as source text; it is parsed
//! and compiled by `Model::compile` (see `expr`).

use crate::elements::ParameterId;

#[derive(Debug, Clone, PartialEq)]
pub struct Driver {
    pub output: ParameterId,
    pub expression: String,
}
