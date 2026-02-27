use std::sync::Arc;
use zohar_db::Auth;
use zohar_net::listen;
use zohar_protocol::token::TokenSigner;

mod auth_srv;

pub async fn serve(addr: String, auth_db: Auth, token_signer: Arc<TokenSigner>) {
    listen(addr, move |stream, server_start, conn_id| {
        auth_srv::handle_conn(
            stream,
            server_start,
            conn_id,
            auth_db.clone(),
            token_signer.clone(),
        )
    })
    .await;
}
