import { useState, useEffect } from 'react';
import {
  Card, Button, Table, Input, Row, Col, Divider, message, Spin, List, Tag,
} from 'antd';
import {
  ScanOutlined, CheckCircleOutlined, HistoryOutlined, FolderOpenOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { open } from '@tauri-apps/plugin-dialog';
import { ocrRecognize, getOcrBatches, confirmOcrResult } from '@/api';
import type { AttendanceRecordInput, OcrBatch } from '@/types';

const OcrCenter: React.FC = () => {
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [month] = useState(dayjs());
  const [recognizing, setRecognizing] = useState(false);
  const [rawText, setRawText] = useState('');
  const [structuredData, setStructuredData] = useState<AttendanceRecordInput[]>([]);
  const [editingCell, setEditingCell] = useState<{ row: number; key: string } | null>(null);
  const [editValue, setEditValue] = useState('');
  const [currentBatchId, setCurrentBatchId] = useState<number | null>(null);
  const [batches, setBatches] = useState<OcrBatch[]>([]);
  const [batchLoading, setBatchLoading] = useState(false);

  const fetchBatches = async () => {
    setBatchLoading(true);
    try {
      const data = await getOcrBatches(month.format('YYYY-MM'));
      setBatches(data);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取历史批次失败: ' + msg);
    } finally {
      setBatchLoading(false);
    }
  };

  useEffect(() => {
    fetchBatches();
  }, []);

  const handleBrowseFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'tiff'] }],
      });
      if (selected) {
        setSelectedFile(selected as string);
        setRawText('');
        setStructuredData([]);
        setCurrentBatchId(null);
      }
    } catch {
      message.error('选择文件失败');
    }
  };

  const handleRecognize = async () => {
    if (!selectedFile) {
      message.warning('请先选择图片文件');
      return;
    }
    setRecognizing(true);
    try {
      const result = await ocrRecognize(selectedFile, month.format('YYYY-MM'));
      setCurrentBatchId(result.batch_id);
      setRawText(result.raw_text);
      setStructuredData(result.records);
      message.success('识别完成');
      fetchBatches();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('识别失败: ' + msg);
    } finally {
      setRecognizing(false);
    }
  };

  const handleCellEdit = (rowIndex: number, key: string, value: unknown) => {
    setEditingCell({ row: rowIndex, key });
    setEditValue(value == null ? '' : String(value));
  };

  const handleCellSave = (rowIndex: number, key: string) => {
    const newData = [...structuredData];
    newData[rowIndex] = { ...newData[rowIndex], [key]: editValue };
    setStructuredData(newData);
    setEditingCell(null);
  };

  const handleConfirm = async () => {
    if (!currentBatchId) return;
    try {
      await confirmOcrResult(currentBatchId, structuredData);
      message.success('已确认入库');
      fetchBatches();
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('入库失败: ' + msg);
    }
  };

  const handleViewBatch = async (batchId: number) => {
    try {
      const batch = batches.find((item) => item.id === batchId);
      if (batch) {
        let records: AttendanceRecordInput[] = [];
        if (batch.parsed_json) {
          records = JSON.parse(batch.parsed_json) as AttendanceRecordInput[];
        }
        setCurrentBatchId(batch.id);
        setRawText(batch.raw_text ?? '');
        setStructuredData(records);
      }
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('获取识别结果失败: ' + msg);
    }
  };

  const statusColorMap: Record<string, string> = {
    '待识别': 'default',
    '识别中': 'processing',
    '已完成': 'success',
    '失败': 'error',
    pending: 'processing',
    confirmed: 'success',
    failed: 'error',
  };

  // Build table columns from structured data keys
  const structColumns = structuredData.length > 0
    ? Object.keys(structuredData[0]).map((key) => ({
        title: key,
        dataIndex: key,
        key,
        render: (text: unknown, _: AttendanceRecordInput, rowIndex: number) =>
          editingCell?.row === rowIndex && editingCell?.key === key ? (
            <Input
              size="small"
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onBlur={() => handleCellSave(rowIndex, key)}
              onPressEnter={() => handleCellSave(rowIndex, key)}
              autoFocus
            />
          ) : (
            <span
              style={{ cursor: 'pointer', padding: '2px 4px', borderRadius: 2 }}
              onClick={() => handleCellEdit(rowIndex, key, text)}
            >
              {text == null || text === '' ? <span style={{ color: '#ccc' }}>点击编辑</span> : String(text)}
            </span>
          ),
      }))
    : [];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">OCR识别中心</span>
      </div>

      <Row gutter={24}>
        <Col xs={24} lg={10}>
          <Card title="上传与识别" style={{ marginBottom: 24 }}>
            <div style={{ textAlign: 'center', padding: '20px 0' }}>
              <Button
                icon={<FolderOpenOutlined />}
                size="large"
                onClick={handleBrowseFile}
                style={{ marginBottom: 16 }}
              >
                选择图片文件
              </Button>
              <p style={{ color: '#999', fontSize: 13 }}>支持 JPG / PNG / BMP 等图片格式</p>
            </div>

            {selectedFile && (
              <div style={{ marginTop: 16, textAlign: 'center' }}>
                <p style={{ color: '#666', marginBottom: 8, wordBreak: 'break-all' }}>
                  已选择: {selectedFile}
                </p>
                <Button
                  type="primary"
                  icon={<ScanOutlined />}
                  onClick={handleRecognize}
                  loading={recognizing}
                  size="large"
                >
                  开始识别
                </Button>
              </div>
            )}
          </Card>
        </Col>

        <Col xs={24} lg={14}>
          <Card title="识别结果" style={{ marginBottom: 24 }}>
            <Spin spinning={recognizing}>
              {rawText ? (
                <>
                  <div style={{ marginBottom: 16 }}>
                    <strong>原始文本：</strong>
                    <pre style={{
                      background: '#f5f5f5',
                      padding: 12,
                      borderRadius: 6,
                      maxHeight: 200,
                      overflow: 'auto',
                      fontSize: 13,
                      whiteSpace: 'pre-wrap',
                    }}>
                      {rawText}
                    </pre>
                  </div>

                  <Divider />

                  <div>
                    <strong>结构化数据：</strong>
                    <span style={{ color: '#999', fontSize: 12, marginLeft: 8 }}>点击单元格可编辑</span>
                    <Table
                      rowKey={(_, idx) => String(idx)}
                      columns={structColumns}
                      dataSource={structuredData}
                      pagination={false}
                      size="small"
                      scroll={{ x: 'max-content' }}
                      style={{ marginTop: 8 }}
                    />
                  </div>

                  <div style={{ marginTop: 16, textAlign: 'right' }}>
                    <Button
                      type="primary"
                      icon={<CheckCircleOutlined />}
                      onClick={handleConfirm}
                    >
                      确认入库
                    </Button>
                  </div>
                </>
              ) : (
                <div style={{ textAlign: 'center', padding: 40, color: '#999' }}>
                  <ScanOutlined style={{ fontSize: 48, marginBottom: 16 }} />
                  <p>请上传图片并点击"开始识别"</p>
                </div>
              )}
            </Spin>
          </Card>
        </Col>
      </Row>

      <Card title={<><HistoryOutlined /> 历史识别批次</>}>
        <List
          loading={batchLoading}
          dataSource={batches}
          locale={{ emptyText: '暂无历史记录' }}
          renderItem={(batch) => (
            <List.Item
              actions={[
                <Button type="link" onClick={() => handleViewBatch(batch.id)}>
                  查看结果
                </Button>,
              ]}
            >
              <List.Item.Meta
                title={batch.batch_name}
                description={`${batch.file_path} | 结果数: ${batch.result_count}`}
              />
              <Tag color={statusColorMap[batch.status] || 'default'}>{batch.status}</Tag>
              <span style={{ color: '#999', marginLeft: 12, fontSize: 12 }}>
                {dayjs(batch.created_at).format('YYYY-MM-DD HH:mm')}
              </span>
            </List.Item>
          )}
        />
      </Card>
    </div>
  );
};

export default OcrCenter;
