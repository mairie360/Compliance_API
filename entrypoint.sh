#!/bin/bash
set -e

# Même dossier que le WORKDIR de development.Dockerfile
cd /usr/src/template # change api name

exec cargo watch --poll -w src -i target -x run
