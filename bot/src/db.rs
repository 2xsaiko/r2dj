pub mod types {
    use sqlx::Type;

    #[derive(Debug, Eq, PartialEq, Type)]
    #[sqlx(type_name = "external_source")]
    #[sqlx(rename_all = "lowercase")]
    pub enum ExternalSource {
        Spotify,
        Youtube,
    }

    #[derive(Debug, Eq, PartialEq, Type)]
    #[sqlx(type_name = "track_provider_type")]
    #[sqlx(rename_all = "lowercase")]
    pub enum TrackProviderType {
        Local,
        Url,
        Spotify,
        Youtube,
    }
}
