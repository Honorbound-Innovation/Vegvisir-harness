#!/usr/bin/env bash
set -euo pipefail

# Show a compact tree of the workspace without noisy hidden artifacts.
# Usage: ./vtree.sh [path]

root="${1:-.}"

find "$root" \
  -path '*/.git' -prune -o \
  -path '*/.vegvisir' -prune -o \
  -print \
  | sed 's#^\./##' \
  | awk '
    {
      n=split($0,a,"/");
      indent="";
      for(i=1;i<n;i++) indent=indent"  ";
      print indent a[n];
    }
  ' \
  | sort
