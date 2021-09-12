#!/bin/bash
emacs -q -l $(dirname $0)/bootstrap.el "$@"
