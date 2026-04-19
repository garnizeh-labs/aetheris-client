// Proto message types come from this crate's own build.rs (prost-only, no service stubs).
#![allow(clippy::missing_errors_doc)]
use crate::auth_proto::{
    ClientMetadata, LoginRequest, LoginResponse, LogoutRequest, LogoutResponse, OtpLoginRequest,
    OtpRequest, OtpRequestAck, login_request,
};
use tonic::codegen::StdError;
use tonic_web_wasm_client::Client;
use tracing::{error, info};

/// Phase 1 WASM-compatible gRPC client for `AuthService`.
///
/// This is a hand-written replacement for the tonic-generated client stub.
pub struct AuthServiceClient<T> {
    inner: tonic::client::Grpc<T>,
}

impl<T> AuthServiceClient<T>
where
    T: tonic::client::GrpcService<tonic::body::Body>,
    T::Error: Into<StdError>,
    T::ResponseBody: tonic::codegen::Body<Data = tonic::codegen::Bytes> + Send + 'static,
    <T::ResponseBody as tonic::codegen::Body>::Error: Into<StdError> + Send,
{
    pub fn new(inner: T) -> Self {
        Self {
            inner: tonic::client::Grpc::new(inner),
        }
    }

    pub async fn request_otp(
        &mut self,
        request: impl tonic::IntoRequest<OtpRequest>,
    ) -> Result<tonic::Response<OtpRequestAck>, tonic::Status> {
        self.inner.ready().await.map_err(|e| {
            tonic::Status::unknown(format!(
                "Service was not ready: {}",
                Into::<StdError>::into(e)
            ))
        })?;
        let codec = tonic_prost::ProstCodec::<OtpRequest, OtpRequestAck>::default();
        let path = tonic::codegen::http::uri::PathAndQuery::from_static(
            "/aetheris.auth.v1.AuthService/RequestOtp",
        );
        self.inner.unary(request.into_request(), path, codec).await
    }

    pub async fn login(
        &mut self,
        request: impl tonic::IntoRequest<LoginRequest>,
    ) -> Result<tonic::Response<LoginResponse>, tonic::Status> {
        self.inner.ready().await.map_err(|e| {
            tonic::Status::unknown(format!(
                "Service was not ready: {}",
                Into::<StdError>::into(e)
            ))
        })?;
        let codec = tonic_prost::ProstCodec::<LoginRequest, LoginResponse>::default();
        let path = tonic::codegen::http::uri::PathAndQuery::from_static(
            "/aetheris.auth.v1.AuthService/Login",
        );
        self.inner.unary(request.into_request(), path, codec).await
    }
    pub async fn logout(
        &mut self,
        request: impl tonic::IntoRequest<LogoutRequest>,
    ) -> Result<tonic::Response<LogoutResponse>, tonic::Status> {
        self.inner.ready().await.map_err(|e| {
            tonic::Status::unknown(format!(
                "Service was not ready: {}",
                Into::<StdError>::into(e)
            ))
        })?;
        let codec = tonic_prost::ProstCodec::<LogoutRequest, LogoutResponse>::default();
        let path = tonic::codegen::http::uri::PathAndQuery::from_static(
            "/aetheris.auth.v1.AuthService/Logout",
        );
        self.inner.unary(request.into_request(), path, codec).await
    }
}

#[allow(clippy::future_not_send)]
pub async fn request_otp(base_url: String, email: String) -> Result<String, String> {
    let redacted = email.split('@').nth(1).map_or("[redacted]", |d| d);
    info!(email_domain = %redacted, "Attempting gRPC OTP request");

    info!(base_url = %base_url, "Initialising gRPC OTP request");
    let web_client = Client::new(base_url.clone());
    let mut client = AuthServiceClient::new(web_client);

    let request = tonic::Request::new(OtpRequest { email });

    match client.request_otp(request).await {
        Ok(response) => {
            let inner = response.into_inner();
            info!(request_id = %inner.request_id, "OTP request successful");
            Ok(inner.request_id)
        }
        Err(e) => {
            let msg = format!("OTP request failed: {}", e.message());
            error!(error = %msg, code = ?e.code(), "gRPC request rejected");
            Err(msg)
        }
    }
}

/// Helper for OTP-based login.
#[allow(clippy::future_not_send)]
pub async fn login_with_otp(
    base_url: String,
    request_id: String,
    code: String,
) -> Result<String, String> {
    info!("Attempting gRPC OTP login");

    info!(base_url = %base_url, "Initialising gRPC OTP login");
    let web_client = Client::new(base_url.clone());
    let mut client = AuthServiceClient::new(web_client);

    let request = tonic::Request::new(LoginRequest {
        method: Some(login_request::Method::Otp(OtpLoginRequest {
            request_id,
            code,
        })),
        metadata: Some(ClientMetadata {
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            platform: "wasm".to_string(),
        }),
    });

    match client.login(request).await {
        Ok(response) => {
            let inner = response.into_inner();
            info!("Login successful!");
            Ok(inner.session_token)
        }
        Err(e) => {
            let msg = format!("Login failed: {}", e.message());
            error!(error = %msg, code = ?e.code(), "gRPC login rejected");
            Err(msg)
        }
    }
}

#[allow(clippy::future_not_send)]
pub async fn logout(base_url: String, session_token: String) -> Result<(), String> {
    info!("Attempting gRPC logout");

    let web_client = Client::new(base_url);
    let mut client = AuthServiceClient::new(web_client);

    let request = tonic::Request::new(LogoutRequest { session_token });

    match client.logout(request).await {
        Ok(response) => {
            let inner = response.into_inner();
            if inner.revoked {
                info!("Logout successful!");
                Ok(())
            } else {
                let msg = "Logout failed: token not revoked by server".to_string();
                error!(error = %msg, "gRPC logout did not revoke token");
                Err(msg)
            }
        }
        Err(e) => {
            let msg = format!("Logout failed: {}", e.message());
            error!(error = %msg, code = ?e.code(), "gRPC logout rejected");
            Err(msg)
        }
    }
}
