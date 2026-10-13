//! A small reader for the exact field of a class R case: an expression in `x`, `y` and `z`.
//!
//! Grammar: numbers, `x`, `y`, `z`, `pi`, `+ - * / ^`, brackets, and the functions
//! `sin`, `cos`, `sinh`, `cosh`, `exp` and `sqrt`. `^` binds tighter than `*` and is right
//! associative. No variable or function outside this list is accepted.

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(usize),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Call(Func, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Sin,
    Cos,
    Sinh,
    Cosh,
    Exp,
    Sqrt,
}

impl Func {
    fn apply(self, v: f64) -> f64 {
        match self {
            Func::Sin => v.sin(),
            Func::Cos => v.cos(),
            Func::Sinh => v.sinh(),
            Func::Cosh => v.cosh(),
            Func::Exp => v.exp(),
            Func::Sqrt => v.sqrt(),
        }
    }
}

impl Expr {
    /// Read the text. The error says what is wrong and where (a byte offset).
    pub fn parse(text: &str) -> Result<Expr, String> {
        let mut p = Parser {
            s: text.as_bytes(),
            at: 0,
        };
        let e = p.sum()?;
        p.skip();
        if p.at < p.s.len() {
            return Err(p.fail("unexpected character"));
        }
        Ok(e)
    }

    pub fn eval(&self, p: [f64; 3]) -> f64 {
        match self {
            Expr::Num(v) => *v,
            Expr::Var(i) => p[*i],
            Expr::Neg(e) => -e.eval(p),
            Expr::Call(f, e) => f.apply(e.eval(p)),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(p), b.eval(p));
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => a.powf(b),
                }
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn fail(&self, what: &str) -> String {
        format!("{what} at offset {}", self.at)
    }

    fn skip(&mut self) {
        while self.s.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.skip();
        let hit = self.s.get(self.at) == Some(&c);
        if hit {
            self.at += 1;
        }
        hit
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.product()?;
        loop {
            let op = if self.eat(b'+') {
                '+'
            } else if self.eat(b'-') {
                '-'
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            let op = if self.eat(b'*') {
                '*'
            } else if self.eat(b'/') {
                '/'
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat(b'-') {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        let base = self.atom()?;
        if self.eat(b'^') {
            return Ok(Expr::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr, String> {
        self.skip();
        let start = self.at;
        match self.s.get(start) {
            Some(b'(') => {
                self.at += 1;
                let e = self.sum()?;
                if self.eat(b')') {
                    Ok(e)
                } else {
                    Err(self.fail("expected `)`"))
                }
            }
            Some(c) if c.is_ascii_digit() || *c == b'.' => {
                while self.s.get(self.at).is_some_and(|c| {
                    c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E')
                        // A sign belongs to the number only inside an exponent.
                        || (matches!(c, b'+' | b'-') && matches!(self.s[self.at - 1], b'e' | b'E'))
                }) {
                    self.at += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.at]).unwrap_or("");
                match text.parse::<f64>() {
                    Ok(v) if v.is_finite() => Ok(Expr::Num(v)),
                    _ => Err(format!("bad number `{text}` at offset {start}")),
                }
            }
            Some(c) if c.is_ascii_alphabetic() => {
                while self.s.get(self.at).is_some_and(u8::is_ascii_alphanumeric) {
                    self.at += 1;
                }
                let name = std::str::from_utf8(&self.s[start..self.at]).unwrap_or("");
                let function = match name {
                    "sin" => Some(Func::Sin),
                    "cos" => Some(Func::Cos),
                    "sinh" => Some(Func::Sinh),
                    "cosh" => Some(Func::Cosh),
                    "exp" => Some(Func::Exp),
                    "sqrt" => Some(Func::Sqrt),
                    _ => None,
                };
                if let Some(f) = function {
                    if !self.eat(b'(') {
                        return Err(self.fail("expected `(`"));
                    }
                    let arg = self.sum()?;
                    if !self.eat(b')') {
                        return Err(self.fail("expected `)`"));
                    }
                    return Ok(Expr::Call(f, Box::new(arg)));
                }
                match name {
                    "x" => Ok(Expr::Var(0)),
                    "y" => Ok(Expr::Var(1)),
                    "z" => Ok(Expr::Var(2)),
                    "pi" => Ok(Expr::Num(std::f64::consts::PI)),
                    _ => Err(format!("unknown name `{name}` at offset {start}")),
                }
            }
            _ => Err(self.fail("expected a number, a name or `(`")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, p: [f64; 3]) -> f64 {
        Expr::parse(text).unwrap().eval(p)
    }

    #[test]
    fn operators_follow_the_usual_order() {
        assert_eq!(at("1 + 2 * 3", [0.0; 3]), 7.0);
        assert_eq!(at("-2 ^ 2", [0.0; 3]), -4.0);
        assert_eq!(at("2 ^ 3 ^ 2", [0.0; 3]), 512.0);
        assert_eq!(at("(1 + 2) * 3 - 4 / 2", [0.0; 3]), 7.0);
    }

    #[test]
    fn variables_and_functions_evaluate() {
        let v = at("1.5e0 * sin(x) * cosh(2 * y) + z", [0.5, 0.25, 2.0]);
        let want = 1.5 * 0.5_f64.sin() * 0.5_f64.cosh() + 2.0;
        assert!((v - want).abs() < 1e-15, "{v} {want}");
        assert!(
            (at("sqrt(2) * pi", [0.0; 3]) - 2.0_f64.sqrt() * std::f64::consts::PI).abs() < 1e-15
        );
        assert_eq!(at("1e-3 + 1E+1", [0.0; 3]), 10.001);
    }

    #[test]
    fn bad_text_is_an_error() {
        for bad in [
            "", "1 +", "foo(1)", "q", "sin 1", "(1", "1 2", "1..2", "sin(x", "1e999",
        ] {
            assert!(Expr::parse(bad).is_err(), "{bad:?} must fail");
        }
    }
}
