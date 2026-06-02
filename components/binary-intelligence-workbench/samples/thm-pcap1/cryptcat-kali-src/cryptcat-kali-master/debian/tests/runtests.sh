#!/bin/sh

# Set up the listener, wait until it's ready.
cryptcat -l -p 4444 -n > new_data &
firstpid=$!
while [ ! -n "$(ss -H -l -t -n sport = :4444)" ]
do
	echo "waiting for listener..."
	sleep 1
done

# Send the message, wait until it's received.
cryptcat 127.0.0.1 4444 < debian/tests/old_data &
secondpid=$!
while [ ! -s new_data ]
do
	echo "waiting for message..."
	sleep 1
done

# Kill the processes.
kill $firstpid $secondpid
wait

# Compare the data
diff new_data debian/tests/old_data
exit $?
