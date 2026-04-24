use reqwest::multipart;
use std::path::PathBuf;

fn transcription_prompt(language: &str) -> Option<&'static str> {
    match language {
        "mixed" | "auto" | "" => Some(
            "Transcribe exactly as spoken. Keep Arabic speech in Arabic script and English words in English. Do not translate between languages.",
        ),
        _ => None,
    }
}

fn validate_groq_config(api_key: &str, model: &str) -> Result<(), String> {
    if api_key.is_empty() {
        return Err("Groq API key not set. Please enter your API key in settings.".to_string());
    }

    if model.trim().is_empty() {
        return Err("Groq model not set. Please enter a Groq model id in settings.".to_string());
    }

    Ok(())
}

pub async fn test_groq_connection(api_key: &str, model: &str) -> Result<String, String> {
    validate_groq_config(api_key, model)?;

    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "https://api.groq.com/openai/v1/models/{}",
            model.trim()
        ))
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await
        .map_err(|e| format!("Groq API request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Groq API error ({}): {}", status, body));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Groq response: {}", e))?;

    let resolved_model = json["id"].as_str().unwrap_or(model).to_string();
    Ok(format!("Connected to Groq model {}", resolved_model))
}

pub async fn transcribe_groq(
    api_key: &str,
    model: &str,
    language: &str,
    audio_path: &PathBuf,
) -> Result<String, String> {
    validate_groq_config(api_key, model)?;

    let audio_bytes =
        std::fs::read(audio_path).map_err(|e| format!("Failed to read audio file: {}", e))?;

    let file_part = multipart::Part::bytes(audio_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;

    let mut form = multipart::Form::new()
        .text("model", model.trim().to_string())
        .text("response_format", "json")
        .part("file", file_part);

    if !matches!(language, "auto" | "mixed" | "") {
        form = form.text("language", language.to_string());
    }

    if let Some(prompt) = transcription_prompt(language) {
        form = form.text("prompt", prompt.to_string());
    }

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.groq.com/openai/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Groq API request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Groq API error ({}): {}", status, body));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Groq response: {}", e))?;

    json["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or("No 'text' field in Groq response".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_api_key() {
        let path = PathBuf::from("/tmp/test.wav");
        let result = transcribe_groq("", "whisper-large-v3", "ar", &path).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("API key not set"));
    }

    #[tokio::test]
    async fn test_empty_model() {
        let result = test_groq_connection("test-key", "").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("model not set"));
    }
}
