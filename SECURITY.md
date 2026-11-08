# Security policy

The BJSONX decoders are written to be safe on hostile input (see `specification/binary-format.md` §5). If you find a way to make any implementation
crash, hang, over-allocate, read out of bounds or accept a malformed blob, please report it **privately** through GitHub's
"Report a vulnerability" (Security → Advisories) instead of a public issue. Include the blob (hex) and the schema if one is needed.
This is a young project (v0.1.x): only the latest release receives fixes.
