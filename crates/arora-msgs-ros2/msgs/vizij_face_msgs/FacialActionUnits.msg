# A face's configuration in FACS: what the face is, with no observation
# (confidence) and no timing. Perception, commands and state reports carry it
# alike.
#
# Sparse: an action unit that is absent is not described — in a perception,
# not detected; in a command, left as it is.

uint8[] codes # FACS action-unit codes
float32[] intensities # [0, 1], one per code
