# Features

Each directory is a feature of Sandbox-mio. With each directory we attempt to 
- `01_ideas.md`: Ideate what the feature is and how we can build it. What technologies we should use and how to balance out the tradeoffs.
- `02_dev.md`: Technical plan for how to implement the feature.
- `03_e2e.md`: Testing plan for checking the feature works and that we don't get regressions as we build more.

Each document is produced by a matching project skill: `feature-ideas` → `01_ideas.md`, `feature-dev` → `02_dev.md`, `feature-implement` → code (keeps `02_dev.md` phase status current), `feature-e2e` → `03_e2e.md`. When resuming a feature, start from the most advanced document already present in the feature's directory. Some features also carry a `spike/` directory with throwaway reproduction scripts referenced from `02_dev.md`.
