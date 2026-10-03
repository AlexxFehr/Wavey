# Stock speaker library

Wavey lists speaker models from a library folder next to the models a
project embeds. The default location is `~/Documents/Wavey Library`; the app
creates it on first launch. Change it with Library ▸ ⋯ ▸ Change Library
Location… (the choice is kept per user).

    Wavey Library/
      Speakers/<Manufacturer>/<id>.waveyspk   converted models (any .waveyspk below the folder is listed)
      GLL/<Manufacturer>/<file>.gll           the manufacturer files they came from
      .wavey-library-index.json              summary cache (rebuilt as files change)

The Library section of the outliner groups devices by manufacturer (the
`manufacturer` field of each model). Devices marked *library* are added to
the project when used: placed (double-click, Place at centre), dropped in
the viewport, or picked in the stack / line-array editors. A project model
of the same id is reused rather than added twice.

## Building it from GLL files

`scripts/build-library.sh ["<library folder>"]` converts the manufacturer
GLL files (looked up in `$GLL_DIR`, default `~/Downloads`) with
`gll2wavey` and copies the originals into `GLL/`. Its manifest lists, per
file, the model id, manufacturer, name, the filter group of each box source
and any extra converter options (`-origin sources` where the file's
reference point is a rigging point at the back of the box).
