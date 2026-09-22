#!/bin/sh
echo $$ > "$BOXR_TEST_ORPHAN_PID"
exec sleep 60
