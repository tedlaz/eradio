use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioStation {
    #[serde(rename = "stationuuid")]
    pub station_uuid: String,
    pub name: String,
    pub url: String,
    #[serde(rename = "url_resolved")]
    pub url_resolved: Option<String>,
    pub tags: Option<String>,
    pub country: Option<String>,
    pub votes: i32,
    pub codec: Option<String>,
    pub bitrate: i32,
    #[serde(rename = "lastcheckok")]
    pub last_check_ok: i32,
}

impl RadioStation {
    pub fn get_stream_url(&self) -> String {
        self.url_resolved
            .clone()
            .unwrap_or_else(|| self.url.clone())
    }
}
