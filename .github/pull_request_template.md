## What

<!-- Describe the public behavior, code, tests, or documentation changed. -->

## Why

<!-- Explain the user-visible problem or capability this change addresses. -->

## Related issue (optional for a small, self-contained fix)

<!-- Use `Closes #N` when this pull request fully resolves an existing issue. -->

## How

<!-- Summarize the implementation at the depth needed for public review. -->

## Testing

<!--
List the exact commands run and their results. Use every applicable row in the
[CONTRIBUTING.md validation table] before opening the pull request.
-->

```text

```

## Compatibility boundary

<!--
State the covered .doc/.docx producers, read/write/edit/render paths, preservation
contract, and any intentionally deferred or unsupported cases.
-->

## Checklist

- [ ] This PR and any related issue contain every decision needed to understand
      the change; review does not depend on a private/internal or BMad artifact.
- [ ] No confidential/private document, credential, personal data, unlicensed
      fixture, `_bmad-output/`, or personal AI work artifact is included.
- [ ] I understand and have reviewed every submitted change, including any
      AI-assisted code or prose.
- [ ] I ran the common gate and every applicable command from
      the [CONTRIBUTING.md validation table], and recorded the results above.
- [ ] New or changed behavior has a focused regression test using synthetic or
      clearly licensed public data.
- [ ] `.doc` format changes cite the relevant [MS-DOC] or [MS-CFB] section;
      `.docx` changes cite the relevant [ECMA-376] section.
- [ ] Public API and behavior changes include corresponding documentation.
- [ ] Any deliberately unsupported remainder is stated above or in the linked
      issue.

## Reviewer notes

<!-- Call out parsing edge cases, preservation risks, trade-offs, or follow-ups. -->

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/ccd7b486-7881-484c-a137-51170af7cc22
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/53989ce4-7b05-4f8d-829b-d08d6148375b
[ECMA-376]: https://ecma-international.org/publications-and-standards/standards/ecma-376/
[CONTRIBUTING.md validation table]: https://github.com/HyunjoJung/rwml/blob/main/CONTRIBUTING.md#validation-by-change-area
