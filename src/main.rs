use mongodb::{
    bson::oid::ObjectId,
    options::{ClientOptions, FindOneAndUpdateOptions},
    Client, Collection,
};
use clap::{App, Arg};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,io,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    time::{self, Duration},
};
use walkdir::WalkDir;
use futures::stream::TryStreamExt;
use chrono::{DateTime, Utc};
use mongodb::bson::doc;

#[derive(Serialize, Deserialize, Debug)]
struct FileDocument {
    #[serde(skip_serializing_if = "Option::is_none")]
    _id: Option<ObjectId>,
    name: String,
    content: String,
    last_synced: String,
    hash: String,
}

async fn hash_file_content(content: &String) -> io::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

async fn read_file_to_string(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut content = String::new();
    file.read_to_string(&mut content).await?;
    Ok(content)
}

async fn scan_directory(dir: &PathBuf, ignored_dirs: &[&str], cache: &mut HashMap<String, String>) -> io::Result<Vec<FileDocument>> {
    let mut files = Vec::new();
    let base_dir = dir.canonicalize()?;
    log::info!("Scanned {} files.", files.len());
    for entry in WalkDir::new(&base_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && !ignored_dirs.iter().any(|&d| e.path().to_str().map_or(false, |p| p.contains(d))))
    {
        let path = entry.path();
        let relative_path = path.strip_prefix(&base_dir).unwrap_or(path).to_string_lossy().to_string();
        let content = read_file_to_string(path).await?;
        let hash = hash_file_content(&content).await?;
        if let Some(existing_hash) = cache.get(&relative_path) {
            if existing_hash == &hash {
                continue;
            }
        }
        log::debug!("Processing file: {:?}", entry.path());
        cache.insert(relative_path.clone(), hash.clone());
        files.push(FileDocument {
            _id: None,
            name: relative_path,
            content,
            last_synced: DateTime::<Utc>::from(SystemTime::now()).to_rfc3339(),
            hash,
        });
    }
    log::info!("Scanned {} files.", files.len());
    Ok(files)
}

async fn sync_files_to_mongodb(
    collection: &Collection<FileDocument>,
    files: Vec<FileDocument>,
    cache: &HashMap<String, String>,
) -> mongodb::error::Result<()> {
    log::info!("Syncing files to MongoDB...");
    for file in files {
        let filter = doc! { "name": &file.name };
        let update = doc! {
            "$set": {
                "content": &file.content,
                "last_synced": &file.last_synced,
                "hash": &file.hash,
            }
        };
        log::info!("Updated or inserted document for file: {}", file.name);
        let options = FindOneAndUpdateOptions::builder().upsert(true).build();
        collection.find_one_and_update(filter, update, options).await?;
    }

    // Handle deleted files
    let mut cursor = collection.find(doc! {}, None).await?;
    while let Some(result) = cursor.try_next().await? {
        let doc_name: String = result.name.clone();
        if !cache.contains_key(&doc_name) {
            collection.delete_one(doc! { "name": doc_name }, None).await?;
        }
    }
    log::info!("Completed syncing files to MongoDB.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("DevAssist CLI Companion")
        .version("1.0")
        .author("Souvik Mukherjee")
        .about("Synchronizes files from a directory to MongoDB")
        .arg(Arg::with_name("project_name")
            .short('p')
            .long("project")
            .value_name("PROJECT")
            .help("Sets the project name for the MongoDB collection")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name("directory_path")
            .short('d')
            .long("directory")
            .value_name("DIRECTORY")
            .help("Sets the directory path to scan")
            .takes_value(true)
            .required(true))
        .get_matches();

    env_logger::init();
    // Gets a value for config if supplied by user, or defaults
    let project_name = matches.value_of("project_name").unwrap();
    let directory_path = matches.value_of("directory_path").unwrap();

    // Get mongodb uri from environment
    let mongodb_uri = std::env::var("MONGODB_URI").expect("MONGODB_URI must be set");
    let client_options = ClientOptions::parse(mongodb_uri).await?;
    let client = Client::with_options(client_options)?;
    let db = client.database("code_sync");
    let collection = db.collection::<FileDocument>(project_name);

    let ignored_dirs = vec![".env", "output", "dist", "target", "build"];
    let mut cache = HashMap::new();

    loop {
        println!("Scanning directory: {:?}", directory_path);
        let files = scan_directory(&PathBuf::from(directory_path), &ignored_dirs, &mut cache).await?;
        if !files.is_empty() {
            sync_files_to_mongodb(&collection, files, &cache).await?;
            println!("Files synchronized to MongoDB.");
        } else {
            println!("No new or modified files to send.");
        }
        time::sleep(Duration::from_secs(30)).await;
    }
}
