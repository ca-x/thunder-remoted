# thunder-remoted

Remote download controller for Xunlei (迅雷) NAS devices, built on
[thunder-task-rs](https://github.com/lib-x/thunder-task-rs).

```
thunder-remoted login        # scan a QR code in the Xunlei app to sign in
thunder-remoted devices      # list devices authorized for your account
thunder-remoted analyze "magnet:?xt=..."     # preview a link
thunder-remoted add "magnet:?xt=..." -d nas   # submit to a device
thunder-remoted list [--all]
thunder-remoted task pause|resume|delete <ids...>
```

Credentials come from `--client-id` / `--client-secret` or the
`THUNDER_CLIENT_ID` / `THUNDER_CLIENT_SECRET` environment variables. The
login session is stored in the OS config dir (`~/.config/thunder-remoted/`
on Linux) and refreshed automatically.

This tool talks to the same cloud API the official clients use, with your
own account and the official OAuth device flow. Server-side account limits
apply unchanged; nothing here bypasses them.

## Build

```
cargo build --release
```

Docker:

```
docker build -t thunder-remoted .
docker run --rm -it -v $PWD:/data thunder-remoted devices
```

## License

MIT
