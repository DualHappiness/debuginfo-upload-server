use axum::{
    body::StreamBody,
    extract::{DefaultBodyLimit, Path},
    response::IntoResponse,
};
use clap::Parser;
use std::sync::RwLock;
use tokio_util::io::ReaderStream;

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

const MAX_SAVE_TIME: u64 = 60 * 60 * 24 * 30;

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
            let path = std::path::Path::new(&OPT.read().unwrap().output).join(&filename);
            tokio::fs::write(&path, data).await.map_err(handle_error)?;
            // TODO(dualwu): improve with storage, incase of not remove file
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(MAX_SAVE_TIME)).await;
                tracing::info!("remove {}", &filename);
                tokio::fs::remove_file(path).await.map_err(|err| {
                    tracing::error!("remove {:?} failed, {:?}", filename, err);
                    err
                })
            });
        }
    }
    Ok("success")
}

async fn download(Path(filename): Path<String>) -> impl IntoResponse {
    let filepath = std::path::Path::new(&OPT.read().unwrap().output).join(&filename);
    let file = match tokio::fs::File::open(&filepath).await {
        Ok(file) => file,
        Err(err) => {
            tracing::error!("open {:?} failed, {:?}", filename, err);
            return Err((axum::http::StatusCode::NOT_FOUND, "not found"));
        }
    };
    let body = StreamBody::new(ReaderStream::new(file));
    let headers = axum::http::HeaderMap::from_iter(vec![
        (
            axum::http::header::CONTENT_DISPOSITION,
            axum::http::HeaderValue::from_str(&format!("attachment; filename={}", filename))
                .unwrap(),
        ),
        (
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_str("application/octet-stream").unwrap(),
        ),
    ]);
    Ok((headers, body))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    *OPT.write().unwrap() = Options::parse();
    let output = OPT.read().unwrap().output.clone();
    tokio::fs::create_dir_all(output).await.unwrap();

    let app = axum::Router::new()
        .route("/debuginfod", axum::routing::post(upload))
        .route("/download/:filename", axum::routing::get(download))
        .layer(DefaultBodyLimit::disable());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], OPT.read().unwrap().port));
    tracing::info!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
