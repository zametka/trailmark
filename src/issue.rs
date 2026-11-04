#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub path: String,
    pub line: usize,
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
}

impl Problem {
    pub fn error(
        code: &'static str,
        path: impl Into<String>,
        line: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            line,
            code,
            severity: Severity::Error,
            message: message.into(),
        }
    }

    pub fn warning(
        code: &'static str,
        path: impl Into<String>,
        line: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            line,
            code,
            severity: Severity::Warning,
            message: message.into(),
        }
    }

    pub fn render(&self) -> String {
        let where_ = if self.path.is_empty() {
            String::new()
        } else if self.line == 0 {
            format!("{}: ", self.path)
        } else {
            format!("{}:{}: ", self.path, self.line)
        };
        format!(
            "{where_}{} {}: {}",
            self.code,
            self.severity.as_str(),
            self.message
        )
    }
}

pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
