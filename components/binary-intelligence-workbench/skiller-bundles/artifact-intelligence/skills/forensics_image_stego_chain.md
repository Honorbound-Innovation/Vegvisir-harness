# forensics.image_stego_chain

Image steganography and metadata chain analysis

Use this skill for image artifacts that may hide evidence in EXIF/XMP metadata, appended/trailing data, steganographic payloads, bit planes, comments, resources, nested archives, or clue/password chains.

The skill emphasizes non-destructive image handling, metadata extraction, file carving, stego-tool use with candidate passphrases, and systematic progression into embedded archives or ciphers. It avoids assuming that visual steganography is required when metadata or trailing data is sufficient.

## Guardrails

- Do not modify originals.
- Do not execute embedded payloads during extraction.
- Respect licensed/private images and authorization scope.
- Treat found passwords/flags as sensitive and avoid durable memory writes.
