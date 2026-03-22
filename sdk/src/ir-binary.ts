/**
 * AppScale Binary IR Encoder/Decoder
 *
 * Converts between the SDK's JSON-style IrBatch/IrCommand objects and
 * FlatBuffers binary format for zero-copy transport to the Rust core.
 *
 * Usage:
 *   import { encodeBatch, decodeBatch } from './ir-binary';
 *
 *   // Encode: JSON IrBatch → Uint8Array (for sending to Rust)
 *   const bytes = encodeBatch(batch);
 *
 *   // Decode: Uint8Array → JSON IrBatch (for DevTools / debugging)
 *   const batch = decodeBatch(bytes);
 */

import * as flatbuffers from 'flatbuffers';
import type { IrBatch as JsonIrBatch, IrCommand as JsonIrCommand } from './types';

// Generated FlatBuffers types
import { IrBatch } from './generated/app-scale/ir/ir-batch';
import { IrCommand } from './generated/app-scale/ir/ir-command';
import { Command } from './generated/app-scale/ir/command';
import { CreateNode } from './generated/app-scale/ir/create-node';
import { UpdateProps } from './generated/app-scale/ir/update-props';
import { UpdateStyle } from './generated/app-scale/ir/update-style';
import { AppendChild } from './generated/app-scale/ir/append-child';
import { InsertBefore } from './generated/app-scale/ir/insert-before';
import { RemoveChild } from './generated/app-scale/ir/remove-child';
import { SetRoot } from './generated/app-scale/ir/set-root';
import { LayoutStyle } from './generated/app-scale/ir/layout-style';
import { PropsDiff } from './generated/app-scale/ir/props-diff';
import { PropEntry } from './generated/app-scale/ir/prop-entry';
import { PropValueUnion } from './generated/app-scale/ir/prop-value-union';
import { StringVal } from './generated/app-scale/ir/string-val';
import { FloatVal } from './generated/app-scale/ir/float-val';
import { IntVal } from './generated/app-scale/ir/int-val';
import { BoolVal } from './generated/app-scale/ir/bool-val';
import { ColorVal } from './generated/app-scale/ir/color-val';
import { Color } from './generated/app-scale/ir/color';
import { Dimension } from './generated/app-scale/ir/dimension';
import { DimensionType } from './generated/app-scale/ir/dimension-type';
import { Edges } from './generated/app-scale/ir/edges';
import { ViewType } from './generated/app-scale/ir/view-type';
import { Display } from './generated/app-scale/ir/display';
import { Position } from './generated/app-scale/ir/position';
import { FlexDirection } from './generated/app-scale/ir/flex-direction';
import { FlexWrap } from './generated/app-scale/ir/flex-wrap';
import { JustifyContent } from './generated/app-scale/ir/justify-content';
import { AlignItems } from './generated/app-scale/ir/align-items';
import { Overflow } from './generated/app-scale/ir/overflow';

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// View type string → FlatBuffers enum mapping
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const VIEW_TYPE_MAP: Record<string, ViewType> = {
  Container: ViewType.Container,
  Text: ViewType.Text,
  TextInput: ViewType.TextInput,
  Image: ViewType.Image,
  ScrollView: ViewType.ScrollView,
  Button: ViewType.Button,
  Switch: ViewType.Switch,
  Slider: ViewType.Slider,
  ActivityIndicator: ViewType.ActivityIndicator,
  DatePicker: ViewType.DatePicker,
  Modal: ViewType.Modal,
  BottomSheet: ViewType.BottomSheet,
  MenuBar: ViewType.MenuBar,
  TitleBar: ViewType.TitleBar,
};

const VIEW_TYPE_REVERSE: Record<number, string> = {};
for (const [name, val] of Object.entries(VIEW_TYPE_MAP)) {
  VIEW_TYPE_REVERSE[val] = name;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Layout enum string → FlatBuffers enum mappings
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const DISPLAY_MAP: Record<string, Display> = {
  flex: Display.Flex, grid: Display.Grid, none: Display.None,
};
const POSITION_MAP: Record<string, Position> = {
  relative: Position.Relative, absolute: Position.Absolute,
};
const FLEX_DIR_MAP: Record<string, FlexDirection> = {
  column: FlexDirection.Column, row: FlexDirection.Row,
  'column-reverse': FlexDirection.ColumnReverse, 'row-reverse': FlexDirection.RowReverse,
};
const FLEX_WRAP_MAP: Record<string, FlexWrap> = {
  nowrap: FlexWrap.NoWrap, wrap: FlexWrap.Wrap, 'wrap-reverse': FlexWrap.WrapReverse,
};
const JUSTIFY_MAP: Record<string, JustifyContent> = {
  'flex-start': JustifyContent.FlexStart, 'flex-end': JustifyContent.FlexEnd,
  center: JustifyContent.Center, 'space-between': JustifyContent.SpaceBetween,
  'space-around': JustifyContent.SpaceAround, 'space-evenly': JustifyContent.SpaceEvenly,
};
const ALIGN_MAP: Record<string, AlignItems> = {
  'flex-start': AlignItems.FlexStart, 'flex-end': AlignItems.FlexEnd,
  center: AlignItems.Center, stretch: AlignItems.Stretch, baseline: AlignItems.Baseline,
};
const OVERFLOW_MAP: Record<string, Overflow> = {
  visible: Overflow.Visible, hidden: Overflow.Hidden, scroll: Overflow.Scroll,
};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Encode: JSON IrBatch → FlatBuffers binary
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function encodeBatch(batch: JsonIrBatch): Uint8Array {
  const builder = new flatbuffers.Builder(1024);

  const cmdOffsets: flatbuffers.Offset[] = [];
  for (const cmd of batch.commands) {
    const offset = encodeCommand(builder, cmd);
    if (offset !== null) {
      cmdOffsets.push(offset);
    }
  }

  const cmdsVec = IrBatch.createCommandsVector(builder, cmdOffsets);

  IrBatch.startIrBatch(builder);
  IrBatch.addCommitId(builder, BigInt(batch.commit_id));
  IrBatch.addTimestampMs(builder, batch.timestamp_ms);
  IrBatch.addCommands(builder, cmdsVec);
  const batchOffset = IrBatch.endIrBatch(builder);

  builder.finish(batchOffset);
  return builder.asUint8Array();
}

function encodeCommand(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset | null {
  switch (cmd.type) {
    case 'create':
      return encodeCreateNode(builder, cmd);
    case 'update_props':
      return encodeUpdateProps(builder, cmd);
    case 'update_style':
      return encodeUpdateStyle(builder, cmd);
    case 'append_child':
      return encodeAppendChild(builder, cmd);
    case 'insert_before':
      return encodeInsertBefore(builder, cmd);
    case 'remove_child':
      return encodeRemoveChild(builder, cmd);
    case 'set_root':
      return encodeSetRoot(builder, cmd);
    default:
      return null;
  }
}

function encodeCreateNode(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset {
  const viewTypeName: string = cmd.view_type ?? 'Container';
  const fbViewType = VIEW_TYPE_MAP[viewTypeName];
  const isCustom = fbViewType === undefined;

  const customTypeOffset = isCustom ? builder.createString(viewTypeName) : 0;
  const propsOffset = cmd.props ? encodeProps(builder, cmd.props) : 0;
  const styleOffset = cmd.style ? encodeLayoutStyle(builder, cmd.style) : 0;

  CreateNode.startCreateNode(builder);
  CreateNode.addId(builder, BigInt(cmd.id));
  CreateNode.addViewType(builder, isCustom ? ViewType.Custom : fbViewType);
  if (isCustom) CreateNode.addCustomType(builder, customTypeOffset as flatbuffers.Offset);
  if (propsOffset) CreateNode.addProps(builder, propsOffset);
  if (styleOffset) CreateNode.addStyle(builder, styleOffset as flatbuffers.Offset);
  const nodeOffset = CreateNode.endCreateNode(builder);

  return IrCommand.createIrCommand(builder, Command.CreateNode, nodeOffset);
}

function encodeUpdateProps(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset {
  const diffOffset = cmd.diff?.changes
    ? encodeProps(builder, cmd.diff.changes)
    : 0;

  UpdateProps.startUpdateProps(builder);
  UpdateProps.addId(builder, BigInt(cmd.id));
  if (diffOffset) UpdateProps.addDiff(builder, diffOffset);
  const offset = UpdateProps.endUpdateProps(builder);

  return IrCommand.createIrCommand(builder, Command.UpdateProps, offset);
}

function encodeUpdateStyle(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset {
  const styleOffset = cmd.style ? encodeLayoutStyle(builder, cmd.style) : 0;

  UpdateStyle.startUpdateStyle(builder);
  UpdateStyle.addId(builder, BigInt(cmd.id));
  if (styleOffset) UpdateStyle.addStyle(builder, styleOffset as flatbuffers.Offset);
  const offset = UpdateStyle.endUpdateStyle(builder);

  return IrCommand.createIrCommand(builder, Command.UpdateStyle, offset);
}

function encodeAppendChild(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset {
  const offset = AppendChild.createAppendChild(builder, BigInt(cmd.parent), BigInt(cmd.child));
  return IrCommand.createIrCommand(builder, Command.AppendChild, offset);
}

function encodeInsertBefore(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset {
  const offset = InsertBefore.createInsertBefore(
    builder, BigInt(cmd.parent), BigInt(cmd.child), BigInt(cmd.before),
  );
  return IrCommand.createIrCommand(builder, Command.InsertBefore, offset);
}

function encodeRemoveChild(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset {
  const offset = RemoveChild.createRemoveChild(builder, BigInt(cmd.parent), BigInt(cmd.child));
  return IrCommand.createIrCommand(builder, Command.RemoveChild, offset);
}

function encodeSetRoot(builder: flatbuffers.Builder, cmd: JsonIrCommand): flatbuffers.Offset {
  const offset = SetRoot.createSetRoot(builder, BigInt(cmd.id));
  return IrCommand.createIrCommand(builder, Command.SetRoot, offset);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Props encoding helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function encodeProps(builder: flatbuffers.Builder, props: Record<string, any>): flatbuffers.Offset {
  const entries: flatbuffers.Offset[] = [];

  for (const [key, value] of Object.entries(props)) {
    const entry = encodePropEntry(builder, key, value);
    if (entry !== null) entries.push(entry);
  }

  const changesVec = PropsDiff.createChangesVector(builder, entries);
  return PropsDiff.createPropsDiff(builder, changesVec);
}

function encodePropEntry(
  builder: flatbuffers.Builder, key: string, value: any,
): flatbuffers.Offset | null {
  const keyOffset = builder.createString(key);
  let valueType: PropValueUnion;
  let valueOffset: flatbuffers.Offset;

  if (value === null || value === undefined) {
    // Null prop — encode as empty StringVal with NONE type
    return null;
  } else if (typeof value === 'string') {
    valueType = PropValueUnion.StringVal;
    const strOffset = builder.createString(value);
    valueOffset = StringVal.createStringVal(builder, strOffset);
  } else if (typeof value === 'boolean') {
    valueType = PropValueUnion.BoolVal;
    valueOffset = BoolVal.createBoolVal(builder, value);
  } else if (typeof value === 'number') {
    if (Number.isInteger(value)) {
      valueType = PropValueUnion.IntVal;
      valueOffset = IntVal.createIntVal(builder, value);
    } else {
      valueType = PropValueUnion.FloatVal;
      valueOffset = FloatVal.createFloatVal(builder, value);
    }
  } else if (typeof value === 'object' && 'r' in value && 'g' in value && 'b' in value) {
    valueType = PropValueUnion.ColorVal;
    const colorOffset = Color.createColor(builder, value.r, value.g, value.b, value.a ?? 1.0);
    ColorVal.startColorVal(builder);
    ColorVal.addValue(builder, colorOffset);
    valueOffset = ColorVal.endColorVal(builder);
  } else {
    // Unsupported prop type — serialize as string fallback
    valueType = PropValueUnion.StringVal;
    const strOffset = builder.createString(JSON.stringify(value));
    valueOffset = StringVal.createStringVal(builder, strOffset);
  }

  return PropEntry.createPropEntry(builder, keyOffset, valueType, valueOffset);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Layout style encoding
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function encodeDimension(builder: flatbuffers.Builder, value: any): flatbuffers.Offset {
  if (value === undefined || value === null || value === 'auto') {
    return Dimension.createDimension(builder, DimensionType.Auto, 0);
  }
  if (typeof value === 'number') {
    return Dimension.createDimension(builder, DimensionType.Points, value);
  }
  if (typeof value === 'object') {
    if ('Points' in value) return Dimension.createDimension(builder, DimensionType.Points, value.Points);
    if ('Percent' in value) return Dimension.createDimension(builder, DimensionType.Percent, value.Percent);
  }
  if (typeof value === 'string' && value.endsWith('%')) {
    return Dimension.createDimension(builder, DimensionType.Percent, parseFloat(value));
  }
  return Dimension.createDimension(builder, DimensionType.Auto, 0);
}

function encodeLayoutStyle(builder: flatbuffers.Builder, style: Record<string, any>): flatbuffers.Offset {
  // Pre-create struct offsets (structs must be created inline before the table starts)
  const widthOff = style.width !== undefined ? encodeDimension(builder, style.width) : 0;
  const heightOff = style.height !== undefined ? encodeDimension(builder, style.height) : 0;
  const minWidthOff = style.minWidth !== undefined ? encodeDimension(builder, style.minWidth) : 0;
  const minHeightOff = style.minHeight !== undefined ? encodeDimension(builder, style.minHeight) : 0;
  const maxWidthOff = style.maxWidth !== undefined ? encodeDimension(builder, style.maxWidth) : 0;
  const maxHeightOff = style.maxHeight !== undefined ? encodeDimension(builder, style.maxHeight) : 0;

  const marginOff = style.margin !== undefined || style.marginTop !== undefined
    ? Edges.createEdges(
        builder,
        style.marginTop ?? style.margin ?? 0,
        style.marginRight ?? style.margin ?? 0,
        style.marginBottom ?? style.margin ?? 0,
        style.marginLeft ?? style.margin ?? 0,
      )
    : 0;

  const paddingOff = style.padding !== undefined || style.paddingTop !== undefined
    ? Edges.createEdges(
        builder,
        style.paddingTop ?? style.padding ?? 0,
        style.paddingRight ?? style.padding ?? 0,
        style.paddingBottom ?? style.padding ?? 0,
        style.paddingLeft ?? style.padding ?? 0,
      )
    : 0;

  LayoutStyle.startLayoutStyle(builder);

  if (style.display !== undefined) LayoutStyle.addDisplay(builder, DISPLAY_MAP[style.display] ?? Display.Flex);
  if (style.position !== undefined) LayoutStyle.addPosition(builder, POSITION_MAP[style.position] ?? Position.Relative);
  if (style.flexDirection !== undefined) LayoutStyle.addFlexDirection(builder, FLEX_DIR_MAP[style.flexDirection] ?? FlexDirection.Column);
  if (style.flexWrap !== undefined) LayoutStyle.addFlexWrap(builder, FLEX_WRAP_MAP[style.flexWrap] ?? FlexWrap.NoWrap);
  if (style.flexGrow !== undefined) LayoutStyle.addFlexGrow(builder, style.flexGrow);
  if (style.flexShrink !== undefined) LayoutStyle.addFlexShrink(builder, style.flexShrink);
  if (style.justifyContent !== undefined) LayoutStyle.addJustifyContent(builder, JUSTIFY_MAP[style.justifyContent] ?? JustifyContent.FlexStart);
  if (style.alignItems !== undefined) LayoutStyle.addAlignItems(builder, ALIGN_MAP[style.alignItems] ?? AlignItems.FlexStart);

  if (widthOff) LayoutStyle.addWidth(builder, widthOff as flatbuffers.Offset);
  if (heightOff) LayoutStyle.addHeight(builder, heightOff as flatbuffers.Offset);
  if (minWidthOff) LayoutStyle.addMinWidth(builder, minWidthOff as flatbuffers.Offset);
  if (minHeightOff) LayoutStyle.addMinHeight(builder, minHeightOff as flatbuffers.Offset);
  if (maxWidthOff) LayoutStyle.addMaxWidth(builder, maxWidthOff as flatbuffers.Offset);
  if (maxHeightOff) LayoutStyle.addMaxHeight(builder, maxHeightOff as flatbuffers.Offset);

  if (style.aspectRatio !== undefined) LayoutStyle.addAspectRatio(builder, style.aspectRatio);
  if (marginOff) LayoutStyle.addMargin(builder, marginOff as flatbuffers.Offset);
  if (paddingOff) LayoutStyle.addPadding(builder, paddingOff as flatbuffers.Offset);
  if (style.gap !== undefined) LayoutStyle.addGap(builder, style.gap);
  if (style.overflow !== undefined) LayoutStyle.addOverflow(builder, OVERFLOW_MAP[style.overflow] ?? Overflow.Visible);

  return LayoutStyle.endLayoutStyle(builder);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Decode: FlatBuffers binary → JSON IrBatch
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

export function decodeBatch(bytes: Uint8Array): JsonIrBatch {
  const buf = new flatbuffers.ByteBuffer(bytes);
  const batch = IrBatch.getRootAsIrBatch(buf);

  const commands: JsonIrCommand[] = [];
  const len = batch.commandsLength();
  for (let i = 0; i < len; i++) {
    const cmd = batch.commands(i);
    if (!cmd) continue;
    const decoded = decodeCommand(cmd);
    if (decoded) commands.push(decoded);
  }

  return {
    commit_id: Number(batch.commitId()),
    timestamp_ms: batch.timestampMs(),
    commands,
  };
}

function decodeCommand(cmd: IrCommand): JsonIrCommand | null {
  switch (cmd.cmdType()) {
    case Command.CreateNode: {
      const node = cmd.cmd(new CreateNode());
      if (!node) return null;
      const vt = node.viewType();
      const viewType = vt === ViewType.Custom
        ? (node.customType() ?? 'Custom')
        : (VIEW_TYPE_REVERSE[vt] ?? 'Container');

      const result: JsonIrCommand = {
        type: 'create',
        id: Number(node.id()),
        view_type: viewType,
        props: {},
        style: {},
      };

      const props = node.props();
      if (props) result.props = decodeProps(props);

      const style = node.style();
      if (style) result.style = decodeLayoutStyle(style);

      return result;
    }

    case Command.UpdateProps: {
      const up = cmd.cmd(new UpdateProps());
      if (!up) return null;
      const diff = up.diff();
      return {
        type: 'update_props',
        id: Number(up.id()),
        diff: { changes: diff ? decodeProps(diff) : {} },
      };
    }

    case Command.UpdateStyle: {
      const us = cmd.cmd(new UpdateStyle());
      if (!us) return null;
      const style = us.style();
      return {
        type: 'update_style',
        id: Number(us.id()),
        style: style ? decodeLayoutStyle(style) : {},
      };
    }

    case Command.AppendChild: {
      const ac = cmd.cmd(new AppendChild());
      if (!ac) return null;
      return {
        type: 'append_child',
        parent: Number(ac.parent()),
        child: Number(ac.child()),
      };
    }

    case Command.InsertBefore: {
      const ib = cmd.cmd(new InsertBefore());
      if (!ib) return null;
      return {
        type: 'insert_before',
        parent: Number(ib.parent()),
        child: Number(ib.child()),
        before: Number(ib.before()),
      };
    }

    case Command.RemoveChild: {
      const rc = cmd.cmd(new RemoveChild());
      if (!rc) return null;
      return {
        type: 'remove_child',
        parent: Number(rc.parent()),
        child: Number(rc.child()),
      };
    }

    case Command.SetRoot: {
      const sr = cmd.cmd(new SetRoot());
      if (!sr) return null;
      return {
        type: 'set_root',
        id: Number(sr.id()),
      };
    }

    default:
      return null;
  }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Props decoding helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

function decodeProps(diff: PropsDiff): Record<string, any> {
  const result: Record<string, any> = {};
  const len = diff.changesLength();

  for (let i = 0; i < len; i++) {
    const entry = diff.changes(i);
    if (!entry) continue;

    const key = entry.key();
    if (!key) continue;

    switch (entry.valueType()) {
      case PropValueUnion.StringVal: {
        const sv = entry.value(new StringVal());
        result[key] = sv?.value() ?? null;
        break;
      }
      case PropValueUnion.FloatVal: {
        const fv = entry.value(new FloatVal());
        result[key] = fv?.value() ?? 0;
        break;
      }
      case PropValueUnion.IntVal: {
        const iv = entry.value(new IntVal());
        result[key] = iv?.value() ?? 0;
        break;
      }
      case PropValueUnion.BoolVal: {
        const bv = entry.value(new BoolVal());
        result[key] = bv?.value() ?? false;
        break;
      }
      case PropValueUnion.ColorVal: {
        const cv = entry.value(new ColorVal());
        const c = cv?.value();
        result[key] = c ? { r: c.r(), g: c.g(), b: c.b(), a: c.a() } : null;
        break;
      }
      default:
        result[key] = null;
    }
  }

  return result;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Layout style decoding
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const DISPLAY_REVERSE: Record<number, string> = { [Display.Flex]: 'flex', [Display.Grid]: 'grid', [Display.None]: 'none' };
const POSITION_REVERSE: Record<number, string> = { [Position.Relative]: 'relative', [Position.Absolute]: 'absolute' };
const FLEX_DIR_REVERSE: Record<number, string> = {
  [FlexDirection.Column]: 'column', [FlexDirection.Row]: 'row',
  [FlexDirection.ColumnReverse]: 'column-reverse', [FlexDirection.RowReverse]: 'row-reverse',
};
const FLEX_WRAP_REVERSE: Record<number, string> = { [FlexWrap.NoWrap]: 'nowrap', [FlexWrap.Wrap]: 'wrap', [FlexWrap.WrapReverse]: 'wrap-reverse' };
const JUSTIFY_REVERSE: Record<number, string> = {
  [JustifyContent.FlexStart]: 'flex-start', [JustifyContent.FlexEnd]: 'flex-end',
  [JustifyContent.Center]: 'center', [JustifyContent.SpaceBetween]: 'space-between',
  [JustifyContent.SpaceAround]: 'space-around', [JustifyContent.SpaceEvenly]: 'space-evenly',
};
const ALIGN_REVERSE: Record<number, string> = {
  [AlignItems.FlexStart]: 'flex-start', [AlignItems.FlexEnd]: 'flex-end',
  [AlignItems.Center]: 'center', [AlignItems.Stretch]: 'stretch', [AlignItems.Baseline]: 'baseline',
};
const OVERFLOW_REVERSE: Record<number, string> = { [Overflow.Visible]: 'visible', [Overflow.Hidden]: 'hidden', [Overflow.Scroll]: 'scroll' };

function decodeDimension(dim: Dimension | null): any {
  if (!dim) return undefined;
  switch (dim.type()) {
    case DimensionType.Auto: return 'auto';
    case DimensionType.Points: return dim.value();
    case DimensionType.Percent: return `${dim.value()}%`;
    default: return 'auto';
  }
}

function decodeLayoutStyle(ls: LayoutStyle): Record<string, any> {
  const result: Record<string, any> = {};

  result.display = DISPLAY_REVERSE[ls.display()] ?? 'flex';
  result.position = POSITION_REVERSE[ls.position()] ?? 'relative';
  result.flexDirection = FLEX_DIR_REVERSE[ls.flexDirection()] ?? 'column';
  result.flexWrap = FLEX_WRAP_REVERSE[ls.flexWrap()] ?? 'nowrap';
  result.flexGrow = ls.flexGrow();
  result.flexShrink = ls.flexShrink();
  result.justifyContent = JUSTIFY_REVERSE[ls.justifyContent()] ?? 'flex-start';
  result.alignItems = ALIGN_REVERSE[ls.alignItems()] ?? 'flex-start';

  const w = decodeDimension(ls.width());
  if (w !== undefined) result.width = w;
  const h = decodeDimension(ls.height());
  if (h !== undefined) result.height = h;
  const minW = decodeDimension(ls.minWidth());
  if (minW !== undefined) result.minWidth = minW;
  const minH = decodeDimension(ls.minHeight());
  if (minH !== undefined) result.minHeight = minH;
  const maxW = decodeDimension(ls.maxWidth());
  if (maxW !== undefined) result.maxWidth = maxW;
  const maxH = decodeDimension(ls.maxHeight());
  if (maxH !== undefined) result.maxHeight = maxH;

  result.aspectRatio = ls.aspectRatio();

  const margin = ls.margin();
  if (margin) {
    result.marginTop = margin.top();
    result.marginRight = margin.right();
    result.marginBottom = margin.bottom();
    result.marginLeft = margin.left();
  }

  const padding = ls.padding();
  if (padding) {
    result.paddingTop = padding.top();
    result.paddingRight = padding.right();
    result.paddingBottom = padding.bottom();
    result.paddingLeft = padding.left();
  }

  result.gap = ls.gap();
  result.overflow = OVERFLOW_REVERSE[ls.overflow()] ?? 'visible';

  return result;
}
