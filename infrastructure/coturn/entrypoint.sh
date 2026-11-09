#!/bin/sh
set -eu
umask 077
# This is operator material, never a user's E2EE key. Required external secret.
test -s /run/secrets/turn-operator.conf
mkdir -p /run/veyra-turn
cat /etc/veyra/turnserver.conf /run/secrets/turn-operator.conf > /run/veyra-turn/turnserver.conf
exec turnserver -c /run/veyra-turn/turnserver.conf
