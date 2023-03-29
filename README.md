# FFMS Segmenter

Reads a video file and makes frame accurate Y4M segments using FFMS2.

## Usage

```bash
$ ffms-segmenter --help
ffms-segmenter 0.1.0

USAGE:
    ffms-segmenter [FLAGS] [OPTIONS] <input-file> [output-folder]

FLAGS:
    -h, --help        Prints help information
    -p, --progress    Disable progress reporting
    -V, --version     Prints version information

OPTIONS:
    -e, --ignore-errors <ignore-errors>     [default: 0]
    -v, --verbose <verbose>                Set FFmpeg verbosity level [default: 0]

ARGS:
    <input-file>       The file to be indexed
    <output-folder>    The output folder. Default to "." if not specified
```
