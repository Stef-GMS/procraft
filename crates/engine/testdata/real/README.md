# Real brush corpus

Do not commit third-party brush files here. Put downloaded files in a local
directory outside the repository and point `PROCRAFT_BRUSH_CORPUS` at it:

```text
$env:PROCRAFT_BRUSH_CORPUS = "D:\brush-corpus"
cargo test -p procraft-engine --test brush_corpus -- --ignored --nocapture
```

The test walks the directory recursively, imports every supported brush file,
prints the brushes, skipped counts, and warnings for each file, and fails if any
file cannot be imported or yields no brushes. Unsupported files such as
`README` or `.DS_Store` are ignored.

Record the source URL and license for every file you add to the local corpus.
Keep proprietary files out of git.
