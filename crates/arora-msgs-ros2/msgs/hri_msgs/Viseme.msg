# One viseme: the visual shape of the mouth for a speech sound, and where it
# falls in the utterance being spoken.
#
# The set is the industry 15-shape one (the Oculus/Meta convention), which is
# what phoneme-to-viseme tables in TTS engines target.

uint8 SIL = 0
uint8 PP = 1
uint8 FF = 2
uint8 TH = 3
uint8 DD = 4
uint8 KK = 5
uint8 CH = 6
uint8 SS = 7
uint8 NN = 8
uint8 RR = 9
uint8 AA = 10
uint8 E = 11
uint8 IH = 12
uint8 OH = 13
uint8 OU = 14

uint8 value # one of the above constants

float32 time # seconds into the utterance
float32 duration # seconds the shape is held
