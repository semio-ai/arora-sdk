#ifndef _ARORA_BUFFER_DESERIALIZE_HPP_
#define _ARORA_BUFFER_DESERIALIZE_HPP_

extern "C" {
  #include <arora/buffers.h>
}

#include <cassert>
#include <cstdint>
#include <optional>
#include <ranges>
#include <string_view>
#include <vector>
#include "types.hpp"
#include "View.hpp"

namespace arora
{
  namespace buffer
  {
    void skip(arora_buffer_reader *const reader, const std::uint8_t type);
    void skipArray(arora_buffer_reader *const reader, const std::uint8_t array_type, const std::uint32_t element_count);

    template<typename T>
    T arora_buffer_reader_get(arora_buffer_reader *const reader);

    template<typename T>
    const T *arora_buffer_reader_get_bulk(arora_buffer_reader *const reader, std::size_t count);

    template<typename T>
    std::optional<T> deserialize(arora_buffer_reader *const reader) {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == arora_buffer_type_of<T>())
        {
          return arora_buffer_reader_get<T>(reader);
        }
        else
        {
          skip(reader, type);
          return std::nullopt;
        }
    }

    template<std::ranges::contiguous_range R>
    std::optional<R> deserialize(arora_buffer_reader *const reader) {
      const std::uint8_t type = arora_buffer_reader_next_type(reader);
      if (type != arora_buffer_type_of<R>)
      {
        skip(reader, type);
        return std::nullopt;
      }

      using T = std::ranges::range_value_t<R>;
      const arora_get_array_result res = arora_buffer_reader_get_array(reader);
      if (res.ty != arora_buffer_type_of<T>)
      {
        skipArray(reader, res.ty, res.element_count);
        return std::nullopt;
      }

      const auto * const data = arora_buffer_reader_get_u8_bulk(reader, res.element_count);
      return R(data, data + res.element_count);
    }

    template<>
    inline bool arora_buffer_reader_get<bool>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_boolean(reader);
    }
    
    template<>
    inline std::uint8_t arora_buffer_reader_get<std::uint8_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_u8(reader);
    }
    
    template<>
    inline std::uint16_t arora_buffer_reader_get<std::uint16_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_u16(reader);
    }
    
    template<>
    inline std::uint32_t arora_buffer_reader_get<std::uint32_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_u32(reader);
    }
    
    template<>
    inline std::uint64_t arora_buffer_reader_get<std::uint64_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_u64(reader);
    }
    template<>
    inline std::int8_t arora_buffer_reader_get<std::int8_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_s8(reader);
    }
    
    template<>
    inline std::int16_t arora_buffer_reader_get<std::int16_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_s16(reader);
    }
    
    template<>
    inline std::int32_t arora_buffer_reader_get<std::int32_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_s32(reader);
    }
    
    template<>
    inline std::int64_t arora_buffer_reader_get<std::int64_t>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_s64(reader);
    }

    template<>
    inline float arora_buffer_reader_get<float>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_r32(reader);
    }
    
    template<>
    inline double arora_buffer_reader_get<double>(arora_buffer_reader *const reader) {
      return arora_buffer_reader_get_r64(reader);
    }
    
    template<>
    inline std::string_view arora_buffer_reader_get<std::string_view>(arora_buffer_reader *const reader) {
          std::uint32_t length = 0;
          const std::uint8_t *const data = arora_buffer_reader_get_string(reader, &length);
          assert(data != nullptr);
          return std::string_view(reinterpret_cast<const char *>(data), length);
    }

    template<>
    inline const std::uint8_t *arora_buffer_reader_get_bulk<std::uint8_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_u8_bulk(reader, count);
    }
    
    template<>
    inline const std::uint16_t *arora_buffer_reader_get_bulk<std::uint16_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_u16_bulk(reader, count);
    }
    
    template<>
    inline const std::uint32_t *arora_buffer_reader_get_bulk<std::uint32_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_u32_bulk(reader, count);
    }
    
    template<>
    inline const std::uint64_t *arora_buffer_reader_get_bulk<std::uint64_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_u64_bulk(reader, count);
    }
    template<>
    inline const std::int8_t *arora_buffer_reader_get_bulk<std::int8_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_s8_bulk(reader, count);
    }
    
    template<>
    inline const std::int16_t *arora_buffer_reader_get_bulk<std::int16_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_s16_bulk(reader, count);
    }
    
    template<>
    inline const std::int32_t *arora_buffer_reader_get_bulk<std::int32_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_s32_bulk(reader, count);
    }
    
    template<>
    inline const std::int64_t *arora_buffer_reader_get_bulk<std::int64_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_s64_bulk(reader, count);
    }

    template<>
    inline const float *arora_buffer_reader_get_bulk<float>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_r32_bulk(reader, count);
    }
    
    template<>
    inline const double *arora_buffer_reader_get_bulk<double>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_r64_bulk(reader, count);
    }
    
    template<>
    inline std::optional<std::vector<std::string_view>> deserialize<std::vector<std::string_view>>(arora_buffer_reader *const reader)
    {
      const std::uint8_t type = arora_buffer_reader_next_type(reader);
      if (type != ARORA_BUFFER_TYPE_ARRAY)
      {
        skip(reader, type);
        return std::nullopt;
      }

      const arora_get_array_result res = arora_buffer_reader_get_array(reader);
      if (res.ty != ARORA_BUFFER_TYPE_STRING)
      {
        skipArray(reader, res.ty, res.element_count);
        return std::nullopt;
      }

      std::vector<std::string_view> result;
      result.reserve(res.element_count);
      for (std::size_t i = 0; i < res.element_count; ++i)
      {
        std::uint32_t length = 0;
        const std::uint8_t *const str = arora_buffer_reader_get_string(reader, &length);
        result.emplace_back(std::string_view(reinterpret_cast<const char *>(str), length));
      }

      return result;
    }
  }
}

#endif