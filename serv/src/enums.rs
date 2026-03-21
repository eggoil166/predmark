use spacetimedb::SpacetimeType;

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketStatus {
    Open,
    Resolved,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Yes,
    No,
}