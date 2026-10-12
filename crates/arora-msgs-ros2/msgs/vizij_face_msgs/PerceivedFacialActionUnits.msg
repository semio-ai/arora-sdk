# A person's action units as a detector observed them: the content, and how
# sure the detector is of each.

std_msgs/Header header
FacialActionUnits facial_action_units
float32[] confidences # [0, 1], one per entry of facial_action_units
