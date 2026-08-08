import { useState, useEffect } from 'react';
import {
  Card, Button, message, Table, Spin, Input, Row, Col, Divider,
} from 'antd';
import {
  FileExcelOutlined, CameraOutlined, CheckCircleOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import { save } from '@tauri-apps/plugin-dialog';
import { open } from '@tauri-apps/plugin-dialog';
import {
  generatePunchCardTemplate, ocrRecognizePunchCard, confirmOcrResult, getOcrSettings,
} from '@/api';
import type { AttendanceRecordInput } from '@/types';

const PunchCard: React.FC = () => {
  const [month] = useState(dayjs());
  const [department, setDepartment] = useState('');
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [recognizing, setRecognizing] = useState(false);
  const [rawText, setRawText] = useState('');
  const [records, setRecords] = useState<AttendanceRecordInput[]>([]);
  const [currentBatchId, setCurrentBatchId] = useState<number | null>(null);
  const [ocrMode, setOcrMode] = useState<'online' | 'local'>('online');

  useEffect(() => {
    getOcrSettings().then(s => setOcrMode(s.ocr_mode as 'online' | 'local')).catch(() => {});
  }, []);

  const handleGenerateTemplate = async () => {
    const monthStr = month.format('YYYY-MM');
    const selected = await save({
      defaultPath: `打卡表-${monthStr}.xlsx`,
      filters: [{ name: 'Excel', extensions: ['xlsx'] }],
    });
    if (!selected) return;
    try {
      await generatePunchCardTemplate(selected as string, monthStr, department);
      message.success('打卡表模板已生成');
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('生成失败: ' + msg);
    }
  };

  const handleBrowseFile = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'bmp'] }],
    });
    if (selected) setSelectedFile(selected as string);
  };

  const handleRecognize = async () => {
    if (!selectedFile) { message.warning('请先选择图片'); return; }
    setRecognizing(true);
    try {
      const result = await ocrRecognizePunchCard(selectedFile, month.format('YYYY-MM'), ocrMode);
      setCurrentBatchId(result.batch_id);
      setRawText(result.raw_text);
      setRecords(result.records);
      message.success(`识别完成，共 ${result.records.length} 条记录`);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('识别失败: ' + msg);
    } finally {
      setRecognizing(false);
    }
  };

  const handleConfirm = async () => {
    if (!currentBatchId) return;
    try {
      await confirmOcrResult(currentBatchId, records);
      message.success('已确认入库');
      setRawText('');
      setRecords([]);
      setCurrentBatchId(null);
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      message.error('入库失败: ' + msg);
    }
  };

  const columns = records.length > 0
    ? Object.keys(records[0]).map((key) => ({ title: key, dataIndex: key, key }))
    : [];

  return (
    <div>
      <div className="page-header">
        <span className="page-title">打卡表管理</span>
      </div>

      <Row gutter={24}>
        <Col xs={24} lg={10}>
          <Card title="生成打卡表模板" style={{ marginBottom: 24 }}>
            <div style={{ display: 'flex', gap: 12, marginBottom: 16, flexWrap: 'wrap' }}>
              <div style={{ flex: 1, minWidth: 120 }}>
                <label style={{ display: 'block', marginBottom: 4, fontSize: 12, color: '#666' }}>部门（可选）</label>
                <Input value={department} onChange={(e) => setDepartment(e.target.value)} placeholder="留空为全部" size="small" />
              </div>
            </div>
            <Button icon={<FileExcelOutlined />} onClick={handleGenerateTemplate} block>
              生成并下载打卡表模板
            </Button>
            <p style={{ color: '#999', fontSize: 12, marginTop: 8 }}>
              打印后让员工签字，拍照后用下方功能扫描识别
            </p>
          </Card>

          <Card title="扫描打卡表" style={{ marginBottom: 24 }}>
            <Button icon={<CameraOutlined />} onClick={handleBrowseFile} block style={{ marginBottom: 12 }}>
              选择打卡表图片
            </Button>
            {selectedFile && (
              <p style={{ color: '#666', fontSize: 12, wordBreak: 'break-all', marginBottom: 12 }}>
                已选择: {selectedFile}
              </p>
            )}
            <Button
              type="primary"
              onClick={handleRecognize}
              loading={recognizing}
              disabled={!selectedFile}
              block
            >
              开始识别
            </Button>
          </Card>
        </Col>

        <Col xs={24} lg={14}>
          <Card title="识别结果" style={{ marginBottom: 24 }}>
            <Spin spinning={recognizing}>
              {rawText ? (
                <>
                  <div style={{ marginBottom: 12 }}>
                    <strong>原始文本：</strong>
                    <pre style={{ background: '#f5f5f5', padding: 8, borderRadius: 4, maxHeight: 150, overflow: 'auto', fontSize: 12 }}>
                      {rawText}
                    </pre>
                  </div>
                  <Divider />
                  <div>
                    <strong>考勤记录：</strong>
                    <span style={{ color: '#999', fontSize: 12, marginLeft: 8 }}>共 {records.length} 条</span>
                    <Table
                      rowKey={(_, idx) => String(idx)}
                      columns={columns}
                      dataSource={records}
                      pagination={false}
                      size="small"
                      scroll={{ x: 'max-content' }}
                      style={{ marginTop: 8 }}
                    />
                  </div>
                  <div style={{ marginTop: 16, textAlign: 'right' }}>
                    <Button type="primary" icon={<CheckCircleOutlined />} onClick={handleConfirm}>
                      确认入库
                    </Button>
                  </div>
                </>
              ) : (
                <div style={{ textAlign: 'center', padding: 40, color: '#999' }}>
                  <CameraOutlined style={{ fontSize: 48, marginBottom: 16 }} />
                  <p>请生成打卡表模板 → 打印签字 → 拍照扫描</p>
                </div>
              )}
            </Spin>
          </Card>
        </Col>
      </Row>
    </div>
  );
};

export default PunchCard;
