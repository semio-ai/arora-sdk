# The ID of the speaker who is uttering the text.
# If the speaker is the system, the ID is SPEAKER_ID_SYSTEM.
# An empty speaker_id is equivalent to setting it to SPEAKER_ID_UNKNOWN
string speaker_id

string SPEAKER_ID_SYSTEM = "__system__"
string SPEAKER_ID_UNKNOWN = "__unknown__"

# The text which is being spoken.
string text

# The localei of the caption, using following format:
# the [ISO 639-1 language code](https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes), 
# lower case, followed by an underscore, 
# followed by the [ISO 3166-1 alpha-2 region code](https://en.wikipedia.org/wiki/ISO_3166-1_alpha-2), 
# upper case.
#
# For example, 'en_GB' specifies British English.
string locale
