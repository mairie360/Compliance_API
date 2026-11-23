#!/bin/bash
set -e

# Même dossier que le WORKDIR de development.Dockerfile
cd /usr/src/compliance

exec cargo watch --poll -w src -i target -x run
