use clap::Parser;
use std::sync::RwLock;

#[derive(Debug, Parser, Default)]
#[clap(author, version, about, long_about = None)]
struct Options {
    #[clap(short, long, value_parser, env = "SERVER_PORT", default_value_t = 8012)]
    port: u16,
    #[clap(
        short,
        long,
        value_parser,
        env = "UPLOAD_DIR",
        default_value = "./uploads"
    )]
    output: String,
}

lazy_static::lazy_static! {
    static ref OPT: RwLock<Options> = RwLock::new(Options::default());
}

fn handle_error(err: impl std::error::Error) -> (axum::http::StatusCode, String) {
    tracing::error!("{:?}", err);
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        format!("{:?}", err),
    )
}

async fn upload(mut mulitpart: axum::extract::Multipart) -> axum::response::Result<&'static str> {
    tracing::info!("upload, {:?}", mulitpart);
    while let Some(field) = mulitpart.next_field().await? {
        if field.name() == Some("file") && field.content_type() == Some("application/octet-stream")
        {
            let filename = field
                .file_name()
                .ok_or((axum::http::StatusCode::BAD_REQUEST, "no filename"))?
                .to_string();
            let data = field.bytes().await.map_err(handle_error)?;
            let path = std::path::Path::new(&OPT.read().unwrap().output).join(filename);
            tokio::fs::write(path, data).await.map_err(handle_error)?;
        }
    }
    Ok("success")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    *OPT.write().unwrap() = Options::parse();
    let output = OPT.read().unwrap().output.clone();
    tokio::fs::create_dir_all(output).await.unwrap();

    let app = axum::Router::new().route("/debuginfod", axum::routing::post(upload));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], OPT.read().unwrap().port));
    tracing::info!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
