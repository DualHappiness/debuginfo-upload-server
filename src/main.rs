use axum::{
    Extension,
    body::Body,
    extract::{DefaultBodyLimit, Path},
    response::{Html, IntoResponse},
};
use clap::Parser;
use std::sync::Arc;
use tokio_util::io::ReaderStream;

#[derive(Debug, Parser, Default, Clone)]
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

    #[clap(
        short,
        long,
        value_parser,
        env = "MAX_SAVE_TIME",
        default_value = "129600"
    )]
    max_save_time: u64,

    #[clap(
        short,
        long,
        value_parser,
        env = "MINIDUMP_DIR",
        default_value = "./uploads/minidumps"
    )]
    minidump_dir: String,

    #[clap(
        short,
        long,
        value_parser,
        env = "MINIDUMP_SYM_DIR",
        default_value = "./uploads/symbols"
    )]
    minidump_sym_dir: String,
}

fn handle_error(err: impl std::error::Error) -> (axum::http::StatusCode, String) {
    tracing::error!("{:?}", err);
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        format!("{:?}", err),
    )
}

async fn upload(
    Extension(opt): Extension<Arc<Options>>,
    mut mulitpart: axum::extract::Multipart,
) -> axum::response::Result<&'static str> {
    tracing::info!("upload, {:?}", mulitpart);
    while let Some(field) = mulitpart.next_field().await? {
        if field.name() == Some("file") && field.content_type() == Some("application/octet-stream")
        {
            let filename = field
                .file_name()
                .ok_or((axum::http::StatusCode::BAD_REQUEST, "no filename"))?
                .to_string();
            let data = field.bytes().await.map_err(handle_error)?;
            let path = std::path::Path::new(&opt.output).join(&filename);
            tokio::fs::write(&path, data).await.map_err(handle_error)?;
        }
    }
    Ok("success")
}

async fn download(
    Extension(opt): Extension<Arc<Options>>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    let filepath = std::path::Path::new(&opt.output).join(&filename);
    let file = match tokio::fs::File::open(&filepath).await {
        Ok(file) => file,
        Err(err) => {
            tracing::error!("open {:?} failed, {:?}", filename, err);
            return Err((axum::http::StatusCode::NOT_FOUND, "not found"));
        }
    };
    let body = Body::from_stream(ReaderStream::new(file));
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

#[tracing::instrument]
async fn upload_minidump_symbol(
    Extension(opt): Extension<Arc<Options>>,
    Path(module_name): Path<String>,
    Path(module_id): Path<String>,
    mut mulitpart: axum::extract::Multipart,
) -> axum::response::Result<&'static str> {
    let module_path = std::path::Path::new(&opt.minidump_sym_dir)
        .join(&module_name)
        .join(&module_id);
    while let Some(field) = mulitpart.next_field().await? {
        if field.name() == Some("file") && field.content_type() == Some("application/octet-stream")
        {
            let filename = field
                .file_name()
                .ok_or((axum::http::StatusCode::BAD_REQUEST, "no filename"))?
                .to_string();
            tracing::info!("upload {:?} to {:?}", filename, module_path);
            let data = field.bytes().await.map_err(handle_error)?;
            let path = module_path.join(&filename);
            tokio::fs::write(&path, data).await.map_err(handle_error)?;
        }
    }
    Ok("success")
}

#[tracing::instrument]
async fn upload_minidump(
    Extension(opt): Extension<Arc<Options>>,
    Path(vehicle_name): Path<String>,
    Path(timestamp): Path<String>,
    mut mulitpart: axum::extract::Multipart,
) -> axum::response::Result<&'static str> {
    let minidump_path = std::path::Path::new(&opt.minidump_dir)
        .join(&vehicle_name)
        .join(&timestamp);
    while let Some(field) = mulitpart.next_field().await? {
        if field.name() == Some("file") && field.content_type() == Some("application/octet-stream")
        {
            let filename = field
                .file_name()
                .ok_or((axum::http::StatusCode::BAD_REQUEST, "no filename"))?
                .to_string();
            tracing::info!("upload {:?} to {:?}", filename, minidump_path);
            let data = field.bytes().await.map_err(handle_error)?;
            let temp_file = std::path::Path::new(&opt.minidump_dir)
                .join(&vehicle_name)
                .join(&timestamp)
                .join(format!("{}.dmp", filename));
            tokio::fs::write(&temp_file, data)
                .await
                .map_err(handle_error)?;

            let command = tokio::process::Command::new("minidump_stackwalk")
                .arg(&temp_file)
                .arg(&opt.minidump_sym_dir)
                .spawn()
                .map_err(handle_error)?;
            let output = command.wait_with_output().await.map_err(handle_error)?;

            if !output.status.success() {
                tracing::error!(
                    "minidump {} process status: {}, err: {}",
                    filename,
                    output.status,
                    std::string::String::from_utf8_lossy(&output.stderr)
                );
            }

            let output_path = std::path::Path::new(&opt.minidump_dir)
                .join(&vehicle_name)
                .join(&timestamp)
                .join(format!("{}.minidump", filename));
            tokio::fs::write(output_path, output.stdout)
                .await
                .map_err(handle_error)?;
            tokio::fs::remove_file(&temp_file)
                .await
                .map_err(handle_error)?;
        }
    }
    Ok("success")
}

#[tracing::instrument(skip(opt))]
async fn list_minidump(
    Extension(opt): Extension<Arc<Options>>,
    Path(vehicle_name): Path<String>,
) -> axum::response::Result<Html<String>> {
    let minidump_path = std::path::Path::new(&opt.minidump_dir).join(&vehicle_name);
    let mut dir = tokio::fs::read_dir(minidump_path)
        .await
        .map_err(handle_error)?;
    let mut files = Vec::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        files.push(entry.path().to_string_lossy().to_string());
    }
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Minidump 文件列表</title>
    <style>
        body {{
            font-family: Arial, sans-serif;
            margin: 20px;
            background-color: #f5f5f5;
        }}
        h1 {{
            color: #333;
        }}
        ul {{
            list-style-type: none;
            padding: 0;
        }}
        li {{
            background-color: white;
            margin: 5px 0;
            padding: 10px;
            border-radius: 4px;
            box-shadow: 0 1px 3px rgba(0,0,0,0.1);
        }}
        li:hover {{
            background-color: #f0f0f0;
        }}
    </style>
</head>
<body>
    <h1>Minidump 文件列表 - {}</h1>
    <ul>
{}
    </ul>
</body>
</html>"#,
        vehicle_name,
        files
            .iter()
            .map(|path| format!("        <li>{}</li>", html_escape(path)))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(Html(html))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[tracing::instrument(skip(opt))]
async fn get_minidump(
    Extension(opt): Extension<Arc<Options>>,
    Path(vehicle_name): Path<String>,
    Path(timestamp): Path<String>,
) -> axum::response::Result<Body> {
    let minidump_path = std::path::Path::new(&opt.minidump_dir)
        .join(&vehicle_name)
        .join(&timestamp);
    let file = tokio::fs::File::open(minidump_path)
        .await
        .map_err(handle_error)?;
    let body = Body::from_stream(ReaderStream::new(file));
    Ok(body)
}

#[tracing::instrument]
async fn remove_expired_file(
    path: &std::path::Path,
    max_save_time: std::time::Duration,
) -> anyhow::Result<()> {
    let now = std::time::SystemTime::now();
    let mut dir = tokio::fs::read_dir(path).await?;
    let mut expired_files = Vec::new();
    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        if let Ok(metadata) = entry.metadata().await {
            if let Ok(last_modified) = metadata.modified() {
                if last_modified + max_save_time < now {
                    expired_files.push(path);
                }
            }
        }
    }
    for file in expired_files {
        if let Err(err) = tokio::fs::remove_file(&file).await {
            tracing::error!("remove expired file {:?} failed, {:?}", file, err);
        } else {
            tracing::info!("remove expired file {:?} at {:?}", file, now);
        }
    }
    Ok(())
}

#[tracing::instrument]
async fn file_monitor(
    path: String,
    max_save_time: std::time::Duration,
    sleep_time: std::time::Duration,
) {
    tracing::info!("file monitor start");
    let path = std::path::Path::new(&path);
    loop {
        tokio::time::sleep(sleep_time).await;
        tracing::info!("scan started");
        if let Err(err) = remove_expired_file(path, max_save_time).await {
            tracing::error!("file monitor read dir {:?} failed, {:?}", path, err);
        }
        tracing::info!("scan complete");
    }
    // tracing::info!("file monitor end");
}

#[tracing::instrument]
async fn init_path(
    path: &String,
    max_save_time: std::time::Duration,
    sleep_time: std::time::Duration,
) {
    tokio::fs::create_dir_all(&path)
        .await
        .expect("failed to create output dir");
    tokio::spawn(file_monitor(
        path.clone(),
        max_save_time.clone(),
        sleep_time.clone(),
    ));
    tracing::info!("init path end");
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let opt = Options::parse();

    let max_save_time = std::time::Duration::from_secs(opt.max_save_time);
    let sleep_time = std::time::Duration::from_hours(4);
    init_path(&opt.output, max_save_time, sleep_time).await;
    init_path(&opt.minidump_sym_dir, max_save_time, sleep_time).await;
    init_path(&opt.minidump_dir, max_save_time, sleep_time).await;

    let app = axum::Router::new()
        .route("/debuginfod", axum::routing::post(upload))
        .route("/download/{filename}", axum::routing::get(download))
        .route(
            "/minidump_sym/{module_name}/{module_id}",
            axum::routing::post(upload_minidump_symbol),
        )
        .route(
            "/minidump/${vehicle_name}",
            axum::routing::get(list_minidump),
        )
        .route(
            "/minidump/{vehicle_name}/{timestamp}",
            axum::routing::post(upload_minidump).get(get_minidump),
        )
        .layer(axum::Extension(Arc::new(opt.clone())))
        .layer(DefaultBodyLimit::disable());

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], opt.port));
    tracing::info!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app).await.expect("failed to serve");
}
