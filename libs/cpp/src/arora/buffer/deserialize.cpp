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
    case ARORA_BUFFER_TYPE_ARRAY:
    {
      arora_get_array_result res = arora_buffer_reader_get_array(reader);
      arora::buffer::skipArray(reader, res.ty, res.element_count);
    }
  }
}

void arora::buffer::skipArray(arora_buffer_reader *const reader, const std::uint8_t array_type, const std::uint32_t element_count)
{
  switch (array_type)
  {
    case ARORA_BUFFER_TYPE_U8:
    {
      arora_buffer_reader_get_u8_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_U16:
    {
      arora_buffer_reader_get_u16_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_U32:
    {
      arora_buffer_reader_get_u32_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_U64:
    {
      arora_buffer_reader_get_u64_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_S8:
    {
      arora_buffer_reader_get_s8_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_S16:
    {
      arora_buffer_reader_get_s16_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_S32:
    {
      arora_buffer_reader_get_s32_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_S64:
    {
      arora_buffer_reader_get_s64_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_R32:
    {
      arora_buffer_reader_get_r32_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_R64:
    {
      arora_buffer_reader_get_r64_bulk(reader, element_count);
      break;
    }
    case ARORA_BUFFER_TYPE_STRING:
    {
      for (std::uint32_t i = 0; i < element_count; ++i)
      {
        std::uint32_t len = 0;
        arora_buffer_reader_get_string(reader, &len);
      }
      break;
    }
    case ARORA_BUFFER_TYPE_STRUCTURE:
    {
      arora_buffer_reader_get_structure_field(reader);
      for (std::uint32_t i = 0; i < element_count; ++i)
      {
        std::uint32_t field_count = arora_buffer_reader_get_structure_raw(reader);
        for (std::uint32_t i = 0; i < field_count; ++i)
        {
          arora::buffer::skip(reader, arora_buffer_reader_next_type(reader));
        }
      }
      break;
    }
  }
}