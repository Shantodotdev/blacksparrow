# Configuring retries

The Relay client retries requests that fail with a network error or a 5xx response. Retries use exponential backoff with full jitter, starting at 200 ms and capped at 10 seconds.

## Changing the retry limit

Set `max_retries` when you build the client. The default is 3; set it to 0 to turn retries off.

```rust
let client = Relay::builder()
    .max_retries(5)
    .build()?;
```

## Which errors are retried

| Error | Retried |
| --- | --- |
| Connection reset | Yes |
| HTTP 429 | Yes, after Retry-After |
| HTTP 400 | No |

See [Error handling][1] for how to inspect the final error.
