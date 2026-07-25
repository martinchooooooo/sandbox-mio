# Features

Each directory is a feature of Sandbox-mio. With each directory we attempt to 
- `01_ideas.md`: Ideate what the feature is and how we can build it. What technologies we should use and how to balance out the tradeoffs.
- `02_dev.md`: Technical plan for how to implement the feature.
- `03_e2e.md`: Testing plan for checking the feature works and that we don't get regressions as we build more.

Two further documents are optional and appear only when a feature has something to put in them:
- `04_improvements.md`: Work deferred as out of scope, plus known seam limitations discovered during implementation.
- `05_dev_review.md`: Post-implementation code review findings and the resolution recorded against each one.

The first three documents are the core pipeline; each is produced by a matching project skill: `feature-ideas` → `01_ideas.md`, `feature-dev` → `02_dev.md`, `feature-implement` → code (keeps `02_dev.md` phase status current), `feature-e2e` → `03_e2e.md`. The optional follow-ons have no dedicated skill — every skill in the pipeline appends to `04_improvements.md`, and `05_dev_review.md` is written by a review pass over finished code. When resuming a feature, start from the most advanced core document already present in the feature's directory. Some features also carry a `spike/` directory with throwaway reproduction scripts referenced from `02_dev.md`.
