#ifndef _ARORA_BUFFER_DESERIALIZE_HPP_
#define _ARORA_BUFFER_DESERIALIZE_HPP_

extern "C" {
  #include <arora/buffers.h>
}
#include <arora/optional.hpp>
#include <cassert>
#include <cstdint>
#include <string>
#include <vector>
#include "types.hpp"

namespace arora
{
  namespace buffer
  {
    template<typename T>
    std::enable_if_t<!detail::is_container<T>::value, std::experimental::optional<T>> deserialize(arora_buffer_reader *const reader) noexcept;

    template<typename T>
    T arora_buffer_reader_get(arora_buffer_reader *const reader) noexcept;

    template<>
    inline bool arora_buffer_reader_get<bool>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_boolean(reader);
    }
    
    template<>
    inline std::uint8_t arora_buffer_reader_get<std::uint8_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_u8(reader);
    }
    
    template<>
    inline std::uint16_t arora_buffer_reader_get<std::uint16_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_u16(reader);
    }
    
    template<>
    inline std::uint32_t arora_buffer_reader_get<std::uint32_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_u32(reader);
    }
    
    template<>
    inline std::uint64_t arora_buffer_reader_get<std::uint64_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_u64(reader);
    }
    template<>
    inline std::int8_t arora_buffer_reader_get<std::int8_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_i8(reader);
    }
    
    template<>
    inline std::int16_t arora_buffer_reader_get<std::int16_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_i16(reader);
    }
    
    template<>
    inline std::int32_t arora_buffer_reader_get<std::int32_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_i32(reader);
    }
    
    template<>
    inline std::int64_t arora_buffer_reader_get<std::int64_t>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_i64(reader);
    }

    template<>
    inline float arora_buffer_reader_get<float>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_f32(reader);
    }
    
    template<>
    inline double arora_buffer_reader_get<double>(arora_buffer_reader *const reader) noexcept {
      return arora_buffer_reader_get_f64(reader);
    }
    
    template<>
    inline std::string arora_buffer_reader_get<std::string>(arora_buffer_reader *const reader) noexcept {
          std::uint32_t length = 0;
          const std::uint8_t *const data = arora_buffer_reader_get_string(reader, &length);
          assert(data != nullptr);
          return std::string(reinterpret_cast<const char *>(data), length);
    }
    
    void skip(arora_buffer_reader *const reader, const std::uint8_t type);
    void skip_array(arora_buffer_reader *const reader, const std::uint8_t array_type, const std::uint32_t element_count);

    template<typename T>
    std::enable_if_t<!detail::is_container<T>::value, std::experimental::optional<T>> deserialize(arora_buffer_reader *const reader) noexcept {
        const std::uint8_t type = arora_buffer_reader_next_type(reader);
        if (type == arora_buffer_type_of<T>())
        {
          return arora_buffer_reader_get<T>(reader);
        }
        else
        {
          skip(reader, type);
          return std::experimental::nullopt;
        }
    }

    // Arrays
    template<typename T>
    const T *arora_buffer_reader_get_bulk(arora_buffer_reader *const reader, std::size_t count) {
      auto* data = new T[count];
      for (std::size_t i = 0; i < count; ++i) {
        data[i] = deserialize<T>(reader).value();
      }
      return data;
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
      return arora_buffer_reader_get_i8_bulk(reader, count);
    }
    
    template<>
    inline const std::int16_t *arora_buffer_reader_get_bulk<std::int16_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_i16_bulk(reader, count);
    }
    
    template<>
    inline const std::int32_t *arora_buffer_reader_get_bulk<std::int32_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_i32_bulk(reader, count);
    }
    
    template<>
    inline const std::int64_t *arora_buffer_reader_get_bulk<std::int64_t>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_i64_bulk(reader, count);
    }

    template<>
    inline const float *arora_buffer_reader_get_bulk<float>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_f32_bulk(reader, count);
    }
    
    template<>
    inline const double *arora_buffer_reader_get_bulk<double>(arora_buffer_reader *const reader, std::size_t count) {
      return arora_buffer_reader_get_f64_bulk(reader, count);
    }

    template<typename T>
    std::experimental::optional<std::vector<T>> deserialize_elements(arora_buffer_reader *const reader) noexcept {
      const arora_get_array_result res = arora_buffer_reader_get_array(reader);
      if (res.ty != arora_buffer_type_of<T>())
      {
        skip_array(reader, res.ty, res.element_count);
        return std::experimental::nullopt;
      }

      const auto * const data = arora_buffer_reader_get_bulk<T>(reader, res.element_count);
      return std::vector<T>(data, data + res.element_count);
    }

    template<>
    inline std::experimental::optional<std::vector<std::string>> deserialize_elements<std::string>(arora_buffer_reader *const reader) noexcept
    {
      const arora_get_array_result res = arora_buffer_reader_get_array(reader);
      if (res.ty != ARORA_BUFFER_TYPE_STRING)
      {
        skip_array(reader, res.ty, res.element_count);
        return std::experimental::nullopt;
      }

      std::vector<std::string> result;
      result.reserve(res.element_count);
      for (std::size_t i = 0; i < res.element_count; ++i)
      {
        std::uint32_t length = 0;
        const std::uint8_t *const str = arora_buffer_reader_get_string(reader, &length);
        result.emplace_back(std::string(reinterpret_cast<const char *>(str), length));
      }

      return result;
    }

    template<typename R>
    std::enable_if_t<detail::is_container<R>::value, std::experimental::optional<R>> deserialize(arora_buffer_reader *const reader) noexcept {
      const std::uint8_t type = arora_buffer_reader_next_type(reader);
      if (type != arora_buffer_type_of<R>())
      {
        skip(reader, type);
        return std::experimental::nullopt;
      }
      using T = typename R::value_type;
      return deserialize_elements<T>(reader);
    }
  }
}

#endif