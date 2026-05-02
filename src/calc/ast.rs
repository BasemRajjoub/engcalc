use std::fmt;

// ─────────────────────────────────────────────────────────────────────────────
// AST
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Var(String),
    BinOp(Box<Expr>, BinOp, Box<Expr>),
    UnaryMinus(Box<Expr>),
    /// f(x), sqrt(x), diff(f,x), integrate(f,x,a,b), sum(f,k,a,b)
    Call(String, Vec<Expr>),
    Pow(Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp { Add, Sub, Mul, Div }

// ─────────────────────────────────────────────────────────────────────────────
// Parse error
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ParseError(pub String);
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, "{}", self.0) }
}
pub fn err(s: impl Into<String>) -> ParseError { ParseError(s.into()) }

// ─────────────────────────────────────────────────────────────────────────────
// Tokeniser
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Num(f64),
    Ident(String),
    Unit(String),
    Plus, Minus, Star, Slash, Caret,
    LParen, RParen, Comma, Eq,
    Eof,
}

pub fn tokenise(src: &str) -> Result<Vec<Tok>, ParseError> {
    let mut toks = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => { chars.next(); }
            '#' => break,
            '"' => {
                chars.next();
                let s: String = chars.by_ref().take_while(|&c| c != '"').collect();
                toks.push(Tok::Unit(s));
            }
            '+' => { toks.push(Tok::Plus);   chars.next(); }
            '-' => { toks.push(Tok::Minus);  chars.next(); }
            '*' => { toks.push(Tok::Star);   chars.next(); }
            '/' => { toks.push(Tok::Slash);  chars.next(); }
            '^' => { toks.push(Tok::Caret);  chars.next(); }
            '(' => { toks.push(Tok::LParen); chars.next(); }
            ')' => { toks.push(Tok::RParen); chars.next(); }
            ',' => { toks.push(Tok::Comma);  chars.next(); }
            '=' => { toks.push(Tok::Eq);     chars.next(); }
            '0'..='9' | '.' => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E'
                        || ((c == '+' || c == '-') && s.ends_with(|x| x == 'e' || x == 'E'))
                    {
                        s.push(c); chars.next();
                    } else { break; }
                }
                let v: f64 = s.parse().map_err(|_| err(format!("bad number: {s}")))?;
                toks.push(Tok::Num(v));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' { s.push(c); chars.next(); }
                    else { break; }
                }
                toks.push(Tok::Ident(s));
            }
            other => return Err(err(format!("unexpected char: {other:?}"))),
        }
    }
    toks.push(Tok::Eof);
    Ok(toks)
}

// ─────────────────────────────────────────────────────────────────────────────
// Recursive-descent parser
// ─────────────────────────────────────────────────────────────────────────────

struct Parser { toks: Vec<Tok>, pos: usize }

impl Parser {
    fn new(toks: Vec<Tok>) -> Self { Self { toks, pos: 0 } }
    fn peek(&self) -> &Tok { &self.toks[self.pos] }
    fn next(&mut self) -> Tok { let t = self.toks[self.pos].clone(); self.pos += 1; t }
    fn expect(&mut self, t: &Tok) -> Result<(), ParseError> {
        if self.peek() == t { self.next(); Ok(()) }
        else { Err(err(format!("expected {t:?}, got {:?}", self.peek()))) }
    }

    fn expr(&mut self) -> Result<Expr, ParseError> { self.add() }

    fn add(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.mul()?;
        loop {
            match self.peek() {
                Tok::Plus  => { self.next(); lhs = Expr::BinOp(lhs.into(), BinOp::Add, self.mul()?.into()); }
                Tok::Minus => { self.next(); lhs = Expr::BinOp(lhs.into(), BinOp::Sub, self.mul()?.into()); }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn mul(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.pow()?;
        loop {
            match self.peek() {
                Tok::Star  => { self.next(); lhs = Expr::BinOp(lhs.into(), BinOp::Mul, self.pow()?.into()); }
                Tok::Slash => { self.next(); lhs = Expr::BinOp(lhs.into(), BinOp::Div, self.pow()?.into()); }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn pow(&mut self) -> Result<Expr, ParseError> {
        let base = self.unary()?;
        if matches!(self.peek(), Tok::Caret) {
            self.next();
            let exp = self.pow()?;
            Ok(Expr::Pow(base.into(), exp.into()))
        } else {
            Ok(base)
        }
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        if matches!(self.peek(), Tok::Minus) {
            self.next();
            Ok(Expr::UnaryMinus(self.unary()?.into()))
        } else {
            self.primary()
        }
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Tok::Num(v) => { self.next(); Ok(Expr::Num(v)) }
            Tok::LParen => {
                self.next();
                let e = self.expr()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Tok::Ident(name) => {
                self.next();
                if matches!(self.peek(), Tok::LParen) {
                    self.next();
                    let mut args = vec![self.expr()?];
                    while matches!(self.peek(), Tok::Comma) {
                        self.next();
                        args.push(self.expr()?);
                    }
                    self.expect(&Tok::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            other => Err(err(format!("unexpected token: {other:?}"))),
        }
    }
}

pub fn parse_expr(src: &str) -> Result<Expr, ParseError> {
    let toks = tokenise(src)?;
    let mut p = Parser::new(toks);
    p.expr()
}
