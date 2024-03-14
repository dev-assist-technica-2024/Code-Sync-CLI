# Code Sync CLI Tool

This is a CLI tool to sync code with DevAssist.

### Steps to Install

1. Clone the repository

```bash
git clone 
```

2. Run `cargo build` to build the project

```bash
cargo build
```

3. Create a `.env` file in the root of the project and add the following environment variables

```bash
DEVASSIST_API_KEY="YourDevAssistAPIKey"
```

4. Run `cargo run` to start the project

```bash
RUST_LOG=info cargo run -- --project "YourProjectName" --directory "desired-folder-root-path"
```