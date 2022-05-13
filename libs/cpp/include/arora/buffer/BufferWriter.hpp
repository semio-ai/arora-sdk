#ifndef _ARORA_BUFFERS_BUFFERWRITER_HPP_
#define _ARORA_BUFFERS_BUFFERWRITER_HPP_

#include <arora/buffers.h>
#include <cstdint>
#include <string>

namespace arora
{
  namespace buffers
  {
    class BufferWriter
    {
    public:
      BufferWriter();
      ~BufferWriter();

      void add_u8(const std::uint8_t value);
      void add_u16(const std::uint16_t value);
      void add_u32(const std::uint32_t value);
      void add_u64(const std::uint64_t value);
      void add_i8(const std::int8_t value);
      void add_i16(const std::int16_t value);
      void add_i32(const std::int32_t value);
      void add_i64(const std::int64_t value);
      void add_f32(const float value);
      void add_f64(const double value);
      void add_string(const std::string &value);

      std::uint8_t *finalize(std::size_t length);


    private:
      void *impl_;
    };
  }
}

#endif