# Orin Controller REPL Grammar

```
command        := sequence-cmd | recovery-cmd | fault-cmd | status-cmd | help-cmd

sequence-cmd   := "reboot" [ "now" | delay-arg ]
recovery-cmd   := "recovery" [ "enter" | "exit" | "now" ]
fault-cmd      := "fault" "recover" [ "retries=" integer ]
status-cmd     := "status"
help-cmd       := "help" [ ident ]

delay-arg      := "delay" duration

duration       := integer ("ms" | "s")
integer        := "0" | ("1"…"9" {"0"…"9"})
ident          := ASCII alpha { ASCII alpha | digit | "-" }
```

## Tokens

| Token        | Example      | Notes                               |
|--------------|--------------|-------------------------------------|
| `Ident`      | `reboot`     | ASCII only, case-insensitive match  |
| `Integer`    | `15`         | Up to 32-bit unsigned               |
| `Duration`   | `200ms`      | Parsed to microseconds internally   |
| `Equals`     | `=`          | Key/value separator                 |
| `Eol`        | `\r`, `\n`   | Line terminator                     |

The lexer (`regal`) produces these tokens and hands them to the `winnow` parser, which applies the productions above. Keywords are matched case-insensitively; arguments preserve case for logging.

## Completion Hints

- Tab completion inspects the grammar tree and offers:
  - command keywords at the beginning of a line,
  - subcommands/flags at each subsequent position,
  - enum-like values (`enter`/`exit`/`now`) once their parent keyword is resolved.
- When multiple matches exist, the REPL emits them as a columnized list and leaves the buffer unchanged.

## Responses

- Successful commands echo `OK <action> <summary>` (e.g., `OK reboot duration=1.2s`).
- Parser or execution errors return `ERR <code> <message>`; the line editor rejects invalid characters up front and signals the user with a terminal BEL instead of emitting caret markers.
- The REPL keeps the input prompt on the terminal's bottom line; command output and telemetry messages are written immediately above it using standard VT100 cursor movements.
- `status` emits the current strap states along with the latest power rail reading, control-link state, and relative ages (`rx`, `tx`) for bridge traffic.
- `recovery now` responds with `OK recovery waiting-for-console` immediately and emits a follow-up event once bridge activity releases the REC strap (or a timeout warning if no activity is seen).
