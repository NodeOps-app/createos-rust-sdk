# Checkpoints

Create a sandbox, checkpoint one directory, modify it, restore the checkpoint,
and delete the checkpoint:

```bash
export CREATEOS_API_KEY=skp_...
cargo run --example checkpoints
```

The example always destroys its sandbox before exiting.
