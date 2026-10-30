// Copyright 2026 Seimei Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The driver expression language: a recursive-descent parser that emits a
//! postfix instruction list, and a stack machine that runs it. The language is
//! specified in `docs/specifications/driver-expressions.md`.

pub const MAX_SOURCE_BYTES: usize = 1024;
pub const MAX_STACK: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Const(f32),
    /// Index into the parameter values.
    Param(usize),
    Add,
    Sub,
    Mul,
    Div,
    Neg,
    Abs,
    Min,
    Max,
    Clamp,
    Lerp,
    Sin,
    Cos,
}

impl Op {
    fn stack_delta(self) -> i32 {
        match self {
            Op::Const(_) | Op::Param(_) => 1,
            Op::Neg | Op::Abs | Op::Sin | Op::Cos => 0,
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Min | Op::Max => -1,
            Op::Clamp | Op::Lerp => -2,
        }
    }

    /// The function a call to `name` runs, and how many arguments it takes.
    fn function(name: &str) -> Option<(Op, usize)> {
        match name {
            "abs" => Some((Op::Abs, 1)),
            "min" => Some((Op::Min, 2)),
            "max" => Some((Op::Max, 2)),
            "clamp" => Some((Op::Clamp, 3)),
            "lerp" => Some((Op::Lerp, 3)),
            "sin" => Some((Op::Sin, 1)),
            "cos" => Some((Op::Cos, 1)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprErrorKind {
    TooLong,
    TooDeep,
    Syntax(&'static str),
    UnknownName(String),
    UnknownFunction(String),
    ArgCount {
        function: String,
        expected: usize,
        actual: usize,
    },
}

/// `position` is a byte offset into the source.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprError {
    pub kind: ExprErrorKind,
    pub position: usize,
}

/// Parses `source` and appends its instructions to `ops`. `resolve` maps a
/// parameter name to the index `Op::Param` reads. `ops` is left unchanged on
/// error.
pub fn compile(
    source: &str,
    resolve: impl Fn(&str) -> Option<usize>,
    ops: &mut Vec<Op>,
) -> Result<(), ExprError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ExprError {
            kind: ExprErrorKind::TooLong,
            position: MAX_SOURCE_BYTES,
        });
    }
    let start = ops.len();
    let mut p = Parser {
        src: source.as_bytes(),
        pos: 0,
        ops,
        depth: 0,
        resolve,
    };
    let result = p.expr().and_then(|()| {
        p.ws();
        if p.pos < p.src.len() {
            Err(p.error(ExprErrorKind::Syntax("unexpected character")))
        } else {
            Ok(())
        }
    });
    if result.is_err() {
        ops.truncate(start);
    }
    result
}

/// Runs a compiled expression. A result that is not finite becomes 0.
pub fn eval(ops: &[Op], values: &[f32]) -> f32 {
    let mut st = [0.0f32; MAX_STACK];
    let mut sp = 0;
    for &op in ops {
        match op {
            Op::Const(c) => {
                st[sp] = c;
                sp += 1;
            }
            Op::Param(i) => {
                st[sp] = values[i];
                sp += 1;
            }
            Op::Neg => st[sp - 1] = -st[sp - 1],
            Op::Abs => st[sp - 1] = st[sp - 1].abs(),
            Op::Sin => st[sp - 1] = st[sp - 1].to_radians().sin(),
            Op::Cos => st[sp - 1] = st[sp - 1].to_radians().cos(),
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Min | Op::Max => {
                sp -= 1;
                let (a, b) = (st[sp - 1], st[sp]);
                st[sp - 1] = match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    Op::Min => a.min(b),
                    _ => a.max(b),
                };
            }
            Op::Clamp => {
                sp -= 2;
                // `lo` wins when `lo > hi`.
                st[sp - 1] = st[sp - 1].min(st[sp + 1]).max(st[sp]);
            }
            Op::Lerp => {
                sp -= 2;
                let (a, b, t) = (st[sp - 1], st[sp], st[sp + 1]);
                st[sp - 1] = a + t * (b - a);
            }
        }
    }
    if st[0].is_finite() { st[0] } else { 0.0 }
}

struct Parser<'a, F> {
    src: &'a [u8],
    pos: usize,
    ops: &'a mut Vec<Op>,
    depth: i32,
    resolve: F,
}

type Parsed = Result<(), ExprError>;

impl<F: Fn(&str) -> Option<usize>> Parser<'_, F> {
    fn error(&self, kind: ExprErrorKind) -> ExprError {
        ExprError {
            kind,
            position: self.pos,
        }
    }

    fn ws(&mut self) {
        while self.src.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.src.get(self.pos).copied()
    }

    fn emit(&mut self, op: Op) -> Parsed {
        self.depth += op.stack_delta();
        if self.depth > MAX_STACK as i32 {
            return Err(self.error(ExprErrorKind::TooDeep));
        }
        self.ops.push(op);
        Ok(())
    }

    fn expr(&mut self) -> Parsed {
        self.term()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.pos += 1;
            self.term()?;
            self.emit(if c == b'+' { Op::Add } else { Op::Sub })?;
        }
        Ok(())
    }

    fn term(&mut self) -> Parsed {
        self.unary()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.pos += 1;
            self.unary()?;
            self.emit(if c == b'*' { Op::Mul } else { Op::Div })?;
        }
        Ok(())
    }

    fn unary(&mut self) -> Parsed {
        if self.peek() == Some(b'-') {
            self.pos += 1;
            self.unary()?;
            return self.emit(Op::Neg);
        }
        self.primary()
    }

    fn primary(&mut self) -> Parsed {
        match self.peek() {
            Some(c) if c.is_ascii_digit() => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => self.name_or_call(),
            Some(b'(') => {
                self.pos += 1;
                self.expr()?;
                if self.peek() != Some(b')') {
                    return Err(self.error(ExprErrorKind::Syntax("expected ')'")));
                }
                self.pos += 1;
                Ok(())
            }
            _ => Err(self.error(ExprErrorKind::Syntax("expected an expression"))),
        }
    }

    fn digits(&mut self) {
        while self.src.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
    }

    fn number(&mut self) -> Parsed {
        let start = self.pos;
        self.digits();
        if self.src.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            self.digits();
        }
        if matches!(self.src.get(self.pos), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(self.src.get(self.pos + 1), Some(b'+' | b'-')));
            if self
                .src
                .get(self.pos + 1 + sign)
                .is_some_and(u8::is_ascii_digit)
            {
                self.pos += 1 + sign;
                self.digits();
            }
        }
        let text = std::str::from_utf8(&self.src[start..self.pos]).expect("ascii digits");
        match text.parse::<f32>() {
            Ok(v) if v.is_finite() => self.emit(Op::Const(v)),
            _ => {
                self.pos = start;
                Err(self.error(ExprErrorKind::Syntax("invalid number")))
            }
        }
    }

    fn name_or_call(&mut self) -> Parsed {
        let start = self.pos;
        while self
            .src
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.pos += 1;
        }
        let name = std::str::from_utf8(&self.src[start..self.pos])
            .expect("ascii identifier")
            .to_owned();
        if self.peek() != Some(b'(') {
            return match (self.resolve)(&name) {
                Some(index) => self.emit(Op::Param(index)),
                None => {
                    self.pos = start;
                    Err(self.error(ExprErrorKind::UnknownName(name)))
                }
            };
        }
        let Some((op, expected)) = Op::function(&name) else {
            self.pos = start;
            return Err(self.error(ExprErrorKind::UnknownFunction(name)));
        };
        self.pos += 1;
        let mut actual = 0;
        if self.peek() != Some(b')') {
            loop {
                self.expr()?;
                actual += 1;
                match self.peek() {
                    Some(b',') => self.pos += 1,
                    Some(b')') => break,
                    _ => return Err(self.error(ExprErrorKind::Syntax("expected ',' or ')'"))),
                }
            }
        }
        self.pos += 1;
        if actual != expected {
            self.pos = start;
            return Err(self.error(ExprErrorKind::ArgCount {
                function: name,
                expected,
                actual,
            }));
        }
        self.emit(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 2] = ["A", "B"];

    fn compile_str(src: &str) -> Result<Vec<Op>, ExprError> {
        let mut ops = Vec::new();
        compile(src, |n| NAMES.iter().position(|&x| x == n), &mut ops).map(|()| ops)
    }

    fn run(src: &str) -> f32 {
        eval(&compile_str(src).unwrap(), &[3.0, -2.0])
    }

    fn kind(src: &str) -> ExprErrorKind {
        compile_str(src).unwrap_err().kind
    }

    #[test]
    fn arithmetic_follows_precedence() {
        assert_eq!(run("1 + 2 * 3"), 7.0);
        assert_eq!(run("(1 + 2) * 3"), 9.0);
        assert_eq!(run("8 - 2 - 1"), 5.0);
        assert_eq!(run("8 / 2 / 2"), 2.0);
        assert_eq!(run("-2 * -3"), 6.0);
        assert_eq!(run("--A"), 3.0);
        assert_eq!(run("1e1 + 0.5e-1"), 10.05);
    }

    #[test]
    fn parameters_are_read_by_name() {
        assert_eq!(run("A * 2 - B"), 8.0);
    }

    #[test]
    fn functions() {
        assert_eq!(run("abs(B)"), 2.0);
        assert_eq!(run("min(A, B)"), -2.0);
        assert_eq!(run("max(A, B)"), 3.0);
        assert_eq!(run("clamp(A, 0, 1)"), 1.0);
        assert_eq!(run("clamp(A, 5, 1)"), 5.0);
        assert_eq!(run("lerp(0, 10, 0.25)"), 2.5);
        assert!((run("sin(30)") - 0.5).abs() < 1e-6);
        assert!((run("cos(60)") - 0.5).abs() < 1e-6);
    }

    #[test]
    fn non_finite_results_become_zero() {
        assert_eq!(run("1 / 0"), 0.0);
        assert_eq!(run("0 / 0"), 0.0);
        assert_eq!(run("1e38 * 1e38"), 0.0);
    }

    #[test]
    fn errors_carry_a_position() {
        let e = compile_str("1 + $").unwrap_err();
        assert_eq!(e.position, 4);
        assert!(matches!(e.kind, ExprErrorKind::Syntax(_)));
        assert!(matches!(kind("(1 + 2"), ExprErrorKind::Syntax(_)));
        assert!(matches!(kind("1 2"), ExprErrorKind::Syntax(_)));
        assert!(matches!(kind(""), ExprErrorKind::Syntax(_)));
        assert!(matches!(kind("1e999"), ExprErrorKind::Syntax(_)));
        assert!(matches!(kind("min(1,"), ExprErrorKind::Syntax(_)));
        assert_eq!(
            compile_str("1 + Zed").unwrap_err(),
            ExprError {
                kind: ExprErrorKind::UnknownName("Zed".into()),
                position: 4
            }
        );
        assert_eq!(
            kind("pow(1, 2)"),
            ExprErrorKind::UnknownFunction("pow".into())
        );
        assert_eq!(
            kind("clamp(A, 0)"),
            ExprErrorKind::ArgCount {
                function: "clamp".into(),
                expected: 3,
                actual: 2
            }
        );
        assert!(matches!(kind("abs()"), ExprErrorKind::ArgCount { .. }));
    }

    #[test]
    fn limits() {
        assert_eq!(kind(&"1+".repeat(MAX_SOURCE_BYTES)), ExprErrorKind::TooLong);
        // Right-nested operands keep every pending value on the stack.
        let deep = format!("{}1{}", "1+(".repeat(MAX_STACK), ")".repeat(MAX_STACK));
        assert_eq!(kind(&deep), ExprErrorKind::TooDeep);
        let ok = format!(
            "{}1{}",
            "1+(".repeat(MAX_STACK - 1),
            ")".repeat(MAX_STACK - 1)
        );
        assert_eq!(run(&ok), MAX_STACK as f32);
    }

    #[test]
    fn failed_compile_leaves_ops_unchanged() {
        let mut ops = vec![Op::Neg];
        assert!(compile("1 + ", |_| None, &mut ops).is_err());
        assert_eq!(ops, vec![Op::Neg]);
    }
}
