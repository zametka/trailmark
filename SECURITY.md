# Security

Report a vulnerability through GitHub private vulnerability reporting on https://github.com/zametka/trailmark (Security → Advisories → Report a vulnerability). Do not file a public issue for an unfixed vulnerability.

If GitHub reporting is unavailable, write to Konstantin Gorodinskii at mail@konstantin.io.

## The read guard is not a boundary

`trailmark adapter install claude-code` installs a hook that blocks a few reads and logs others. It is a convenience for coding agents. It is not a security control.

- It fails open on input it does not understand.
- It does not see every way to read a file. The README lists the holes that are known and left as limits.
- The records are ordinary files. Anyone who can read the working tree can read them.
- `TRAIL_ALLOW=brief` in a command is enough to let a blocked history read through.

Do not put a secret in a record and expect the guard to keep it there.
