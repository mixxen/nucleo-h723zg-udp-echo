"""CRC-32C framing, deliberately separate from the three body serializers."""

from codec import CodecError, MAX_BODY, decode as decode_body, encode as encode_body

CRC_MODES = ("off", "on")
MAX_DATAGRAM = MAX_BODY + 8


def checksum(data):
    """Independent bitwise Castagnoli reference; not zlib's IEEE CRC-32."""
    value = 0xFFFFFFFF
    for byte in data:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ (0x82F63B78 if value & 1 else 0)
    return value ^ 0xFFFFFFFF


def seal(body, crc="off"):
    if crc not in CRC_MODES:
        raise ValueError("CRC mode must be off or on")
    if len(body) > MAX_BODY:
        raise CodecError("size_error")
    return body + (f"{checksum(body):08X}".encode("ascii") if crc == "on" else b"")


def open_frame(data, crc="off"):
    if crc not in CRC_MODES:
        raise ValueError("CRC mode must be off or on")
    if len(data) > MAX_BODY + (8 if crc == "on" else 0):
        raise CodecError("size_error")
    if crc == "off":
        return data
    if len(data) < 8 or any(c not in b"0123456789ABCDEF" for c in data[-8:]):
        raise CodecError("crc_error")
    body, trailer = data[:-8], data[-8:]
    if checksum(body) != int(trailer, 16):
        raise CodecError("crc_error")
    return body


def encode(format, message, crc="off"):
    return seal(encode_body(format, message), crc)


def decode(format, data, crc="off"):
    return decode_body(format, open_frame(data, crc))
