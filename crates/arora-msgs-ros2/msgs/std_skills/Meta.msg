# name of the node calling the skill
string caller

# priority of this call. Higher values mean higher priority.
# May be used by the skill implementation to either replace a current skill
# execution with the new request, or reject/ignore the skill call.
#
# Priorities above 200 are reserved for critical system functions.

uint8 LOWEST_PRIORITY = 0
uint8 NORMAL_PRIORITY = 128
uint8 HIGHEST_PRIORITY = 255

uint8 priority

