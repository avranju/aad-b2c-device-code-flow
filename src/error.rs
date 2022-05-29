use std::fmt::Display;

use axum::{http::StatusCode, response::IntoResponse};

pub struct AppError<T: Display>(StatusCode, T);

impl<T> IntoResponse for AppError<T>
where
    T: Display,
{
    fn into_response(self) -> axum::response::Response {
        (self.0, self.1.to_string()).into_response()
    }
}

impl From<url::ParseError> for AppError<url::ParseError> {
    fn from(err: url::ParseError) -> Self {
        AppError(StatusCode::INTERNAL_SERVER_ERROR, err)
    }
}
