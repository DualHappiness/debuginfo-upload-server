use std::{convert::Infallible, path, sync::RwLock};

use bytes::BufMut;
use clap::Parser;
use futures::TryStreamExt;
use warp::{hyper::StatusCode, Filter, Rejection, Reply};

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

async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    let (code, message) = if err.is_not_found() {
        (StatusCode::NOT_FOUND, "Not Found".to_string())
    } else if err.find::<warp::reject::PayloadTooLarge>().is_some() {
        (StatusCode::BAD_REQUEST, "Payload too large".to_string())
    } else {
        tracing::error!("unhandled error: {:?}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error".to_string(),
        )
    };
    tracing::debug!("{} {}", code, message);
    Ok(warp::reply::with_status(message, code))
}

fn reject_err(e: impl std::fmt::Debug) -> Rejection {
    tracing::error!("{:?}", e);
    warp::reject()
}

async fn upload(form: warp::multipart::FormData) -> Result<impl Reply, Rejection> {
    let parts: Vec<::warp::multipart::Part> = form.try_collect().await.map_err(reject_err)?;
    for p in parts {
        tracing::info!("{:?}", p);
        if p.name() == "file" && p.content_type() == Some("application/octet-stream") {
            let filename = p.filename().unwrap().to_string();
            let bytes = p
                .stream()
                .try_fold(Vec::new(), |mut vec, data| {
                    vec.put(data);
                    async move { Ok(vec) }
                })
                .await
                .map_err(reject_err)?;
            let path = path::Path::new(&OPT.read().map_err(reject_err)?.output).join(filename);
            tokio::fs::write(path, bytes).await.map_err(reject_err)?;
            return Ok("success");
        }
    }
    Err(warp::reject())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    *OPT.write().unwrap() = Options::parse();
    tokio::fs::create_dir_all(&OPT.read().unwrap().output)
        .await
        .unwrap();

    let upload = warp::post()
        .and(warp::path("debuginfod"))
        .and(warp::multipart::form().max_length(500 * 1024 * 1024))
        .and_then(upload);

    let download = warp::get()
        .and(warp::path("debuginfod"))
        .and(warp::fs::dir(OPT.read().unwrap().output.clone()));

    let route = upload.or(download).recover(handle_rejection);

    warp::serve(route)
        .run(([0, 0, 0, 0], OPT.read().unwrap().port))
        .await;
}
