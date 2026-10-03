//! Where SMAPI releases are read from. The manager maintains no list of its
//! own: releases and their game ranges come from SMAPI's published sources.

use crate::error::AppResult;
use async_trait::async_trait;
use manager_core::smapi::catalog::SmapiRelease;

#[async_trait]
pub trait SmapiReleaseSourcePort: Send + Sync {
    /// Every published release with a player installer, without game ranges.
    async fn list_releases(&self) -> AppResult<Vec<SmapiRelease>>;
    /// The game range SMAPI declares at `tag`: (minimum, maximum).
    async fn game_range(&self, tag: &str) -> AppResult<(Option<String>, Option<String>)>;
}
