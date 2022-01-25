#include "arora/buffer/deserialize.hpp"


void arora::buffer::skip(arora_buffer_reader *const reader, const std::uint8_t type)
{
  switch (type)
  {
    case ARORA_BUFFER_TYPE_BOOLEAN:
      arora_buffer_reader_get_boolean(reader);
      break;
    case ARORA_BUFFER_TYPE_U8:
      arora_buffer_reader_get_u8(reader);
      break;
    case ARORA_BUFFER_TYPE_U16:
      arora_buffer_reader_get_u16(reader);
      break;
    case ARORA_BUFFER_TYPE_U32:
      arora_buffer_reader_get_u32(reader);
      break;
    case ARORA_BUFFER_TYPE_U64:
      arora_buffer_reader_get_u64(reader);
      break;
    case ARORA_BUFFER_TYPE_S8:
      arora_buffer_reader_get_s8(reader);
      break;
    case ARORA_BUFFER_TYPE_S16:
      arora_buffer_reader_get_s16(reader);
      break;
    case ARORA_BUFFER_TYPE_S32:
      arora_buffer_reader_get_s32(reader);
      break;
    case ARORA_BUFFER_TYPE_S64:
      arora_buffer_reader_get_s64(reader);
      break;
    case ARORA_BUFFER_TYPE_R32:
      arora_buffer_reader_get_r32(reader);
      break;
    case ARORA_BUFFER_TYPE_R64:
      arora_buffer_reader_get_r64(reader);
      break;
    case ARORA_BUFFER_TYPE_STRING:
    {
      std::uint32_t len = 0;
      arora_buffer_reader_get_string(reader, &len);
      break;
    }
    case ARORA_BUFFER_TYPE_ENUMERATION:
    {
      arora_get_enumeration_value_result res = arora_buffer_reader_get_enumeration_value(reader);
      skip(reader, arora_buffer_reader_next_type(reader));
      break;
    }
    case ARORA_BUFFER_TYPE_STRUCTURE:
    {
      arora_get_structure_result res = arora_buffer_reader_get_structure(reader);
      for (std::uint32_t i = 0; i < res.field_count; ++i)
      {
        skip(reader, arora_buffer_reader_next_type(reader));
      }
      break;
    }
  }
}
