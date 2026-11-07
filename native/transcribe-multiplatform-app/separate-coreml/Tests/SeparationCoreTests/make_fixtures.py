#!/usr/bin/env python3
"""Prints the tiny Core ML models CoreMLWindowModelTests holds, as base64. Needs coremltools."""
import base64

from coremltools.models import datatypes
from coremltools.models.neural_network import NeuralNetworkBuilder
from coremltools.proto import FeatureTypes_pb2

SAMPLES = 32


def model(input_shape, output_shape, expand=True):
    builder = NeuralNetworkBuilder(
        [("wave", datatypes.Array(*input_shape))],
        [("tracks", datatypes.Array(*output_shape))],
        disable_rank5_shape_mapping=True,
    )
    each = "wave"
    if expand:
        builder.add_expand_dims("expand", "wave", "expanded", axes=[0])
        each = "expanded"
    builder.add_concat_nd("twice", [each, each], "tracks", axis=0)
    spec = builder.spec
    float32 = FeatureTypes_pb2.ArrayFeatureType.FLOAT32
    spec.description.input[0].type.multiArrayType.dataType = float32
    spec.description.output[0].type.multiArrayType.dataType = float32
    return base64.b64encode(spec.SerializeToString()).decode()


print("twoTracks", model((1, SAMPLES), (2, 1, SAMPLES)))
print("wrongInput", model((SAMPLES,), (2, SAMPLES)))
print("wrongOutput", model((1, SAMPLES), (2, SAMPLES), expand=False))
