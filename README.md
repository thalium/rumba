# Rumba

An MBA simplification library

## Artifacts

Build artifacts using

```
$ docker build -o build .
```

This will produce:
- `build/gamba_res.csv` and `build/rumba_res.csv` which together with `make_graph.py` can be used to generate the time comparison graph.
- `rumba` a standalone binary for testing the MBA simplification
- `test_results.txt` the results of the testcases, with statistics on simplification times as well as success rates.

The datasets are available in `third_party/dataset`.

### Datasets

This project includes the dataset from [GAMBA](https://github.com/DenuvoSoftwareSolutions/GAMBA) (Copyright (c) 2023 Denuvo GmbH, released under GPLv3).

The dataset is located in `third_party/dataset` and is redistributed under its original license.
See the LICENSE.md file in that directory for full terms.

The datasets were preprocessed to unify variable names and file formats.

<!-- ## Using rumba

Rumba can simplify passed MBAs

```
./build/rumba -- "-34*~v1*(v0&v1)-36*~v1*(v0&~v1)+12*~v1*~(v0&v1)+10*~v1*~(v0^v1)-24*~v1*~(v0|v1)-36*~v1*~(v0|~v1)+17*v1*(v0&v1)+18*v1*(v0&~v1)-6*v1*~(v0&v1)-5*v1*~(v0^v1)+12*v1*~(v0|v1)+18*v1*~(v0|~v1)+22*~v1*(v0|v1)-11*v1*(v0|v1)"
``` -->

## License

> Unless otherwise noted, all files in this repository are licensed under the MIT License; files located in `third_party/` are excluded and remain licensed under their original license.

Copyright THALES 2026

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.


