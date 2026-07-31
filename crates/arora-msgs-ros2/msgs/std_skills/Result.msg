
# Error codes
#############

# where meaningful, error codes are aligned with C++11 error codes:
# https://en.cppreference.com/w/cpp/error/errno_macros

# no error
uint8 ROS_ENOERR = 0

# interupted skill execution.
# if a current executing skill if interupted (for instance by a higher-priority
# incoming request), the cancelled call returns this error code.
uint8 ROS_EINTR = 4

# bad message
# the provided parameters to call the skill are invalid and/or missing.
uint8 ROS_EBADMSG = 74

# canceled skill
# if the skill execution is canceled by the caller, this error code is returned.
uint8 ROS_ECANCELED = 125

# not supported
# the requested skill is not supported (for instance, not implemented yet)
uint8 ROS_ENOTSUP = 134

# other error
uint8 ROS_EOTHER = 255

###########

uint8 error_code
string error_msg
