use serde::{Serialize, Serializer};

/// Erro único do backend. Todo comando Tauri devolve `Result<T, AppError>`,
/// e o front recebe `{ kind, message }` para decidir o que mostrar.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Esta ação precisa que o Skill Gap seja aberto como administrador.")]
    RequiresAdmin,

    #[error("Item não encontrado: {0}")]
    NotFound(String),

    #[error("Entrada inválida: {0}")]
    InvalidInput(String),

    #[error("O comando {command} falhou: {detail}")]
    CommandFailed { command: String, detail: String },

    #[error("Você recusou o pedido de permissão de administrador do Windows. Nada foi alterado.")]
    ElevationDenied,

    /// Erro que aconteceu dentro do processo auxiliar elevado.
    #[error("{0}")]
    Helper(String),

    /// O ajuste não serve pra este PC (build, RAM, disco, notebook). Diz o motivo.
    #[error("Indisponível neste PC: {0}")]
    Unavailable(String),

    /// Ajuste de risco alto pedido sem confirmação explícita.
    #[error("“{0}” tem risco alto e precisa de confirmação antes de aplicar.")]
    ConfirmationRequired(String),

    #[error("Recurso disponível apenas no Windows.")]
    #[allow(dead_code)] // usado só fora do Windows (stubs)
    Unsupported,

    #[error("Erro de E/S: {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::RequiresAdmin => "requiresAdmin",
            AppError::NotFound(_) => "notFound",
            AppError::InvalidInput(_) => "invalidInput",
            AppError::CommandFailed { .. } => "commandFailed",
            AppError::ElevationDenied => "elevationDenied",
            AppError::Helper(_) => "helper",
            AppError::Unavailable(_) => "unavailable",
            AppError::ConfirmationRequired(_) => "confirmationRequired",
            AppError::Unsupported => "unsupported",
            AppError::Io(_) => "io",
        }
    }

    /// Acesso negado no registro vira RequiresAdmin, pro front mostrar uma mensagem útil
    /// em vez de "os error 5".
    pub fn from_io(err: std::io::Error) -> Self {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            AppError::RequiresAdmin
        } else {
            AppError::Io(err)
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
