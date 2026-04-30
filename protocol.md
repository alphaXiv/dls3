Message format:
- Header (3 bytes)
    - Tag (u8)
    - Payload size (u16)
- Payload (n bytes)

All numbers are little-endian unless otherwise specified.

Client messages:
- Tag 0: request to open a file. Payload is the object key.
- Tag 1: inform that the victim has closed a file. Payload is the object key.
- Tag 2: update authentication. Payload:
    - 2 bytes: length of new access key ID
    - new access key ID
    - 2 bytes: length of new secret access key
    - new secret access key
    - 2 bytes: length of new session token (0 if there isn't one)
    - new session token

Server messages:
- Tag 0: response to open request. Payload is the errno value (u16, 0 for success).

TODO: add versioning so nothing blows up if an incompatible hook and daemon talk to each other
