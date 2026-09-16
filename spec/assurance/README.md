# Development Assurance metadata

Normative Markdown remains the source of truth. A requirement may include a fenced TOML block whose info string is exactly `requirement` to provide only machine-readable identity/classification metadata:

````markdown
```requirement
id = "REQ-DEV-GITHUB-HOSTED-CI"
kind = "architecture"
criticality = "medium"
domain = "development"
```
````

The surrounding prose remains normative. This convention is metadata, not a new requirements DSL.
