use {
    clap::{Parser, ValueEnum},
    llmweb::{Format, LlmWeb, RunOptions, error::LlmWebError},
    serde_json::{Value, from_str},
    std::{fs, path::PathBuf},
};

/// A CLI tool to extract structured data from webpages using LLMs.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The URL of the webpage to process.
    #[arg()]
    url: String,

    /// Path to the JSON file containing the schema for data extraction.
    #[arg(short, long)]
    schema_file: PathBuf,

    /// The name of the LLM model to use.
    #[arg(short, long, default_value = "gemini-1.5-flash")]
    model: String,

    /// Preprocessing format. Use `image` only with multimodal models.
    #[arg(short, long, value_enum, default_value_t = CliFormat::Html)]
    format: CliFormat,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum CliFormat {
    Html,
    RawHtml,
    Markdown,
    Text,
    Image,
}

impl From<CliFormat> for Format {
    fn from(f: CliFormat) -> Self {
        match f {
            CliFormat::Html => Format::Html,
            CliFormat::RawHtml => Format::RawHtml,
            CliFormat::Markdown => Format::Markdown,
            CliFormat::Text => Format::Text,
            CliFormat::Image => Format::Image,
        }
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), LlmWebError> {
    let args = Args::parse();

    let schema_str =
        fs::read_to_string(&args.schema_file).map_err(|e| LlmWebError::Io(e.to_string()))?;
    let schema: Value = from_str(&schema_str)?;

    let llmweb = LlmWeb::new(&args.model);

    eprintln!("Processing URL: {}", &args.url);
    eprintln!("Using model:    {}", &args.model);
    eprintln!("Format:         {:?}", &args.format);

    let result: Value = llmweb
        .exec_with(&args.url, schema, RunOptions::new(args.format.into()))
        .await?;

    let pretty_json = serde_json::to_string_pretty(&result)?;
    println!("{pretty_json}");

    Ok(())
}
