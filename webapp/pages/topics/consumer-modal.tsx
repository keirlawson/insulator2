import { Chip, Stack, Title, Text, Group, Checkbox, Button, Divider } from "@mantine/core";
import { openModal, useModals } from "@mantine/modals";
import { DateRangePicker, DatePicker, TimeRangeInput, TimeInput } from "@mantine/dates";
import { ConsumerSettingsFrom } from "../../models/kafka";
import { useForm } from "@mantine/form";
import dayjs from "dayjs";
import utc from "dayjs/plugin/utc";
import { startConsumer } from "../../tauri/consumer";

dayjs.extend(utc);

type ConsumerModalProps = {
  clusterId: string;
  topicName: string;
};

export const openConsumerModal = (props: ConsumerModalProps) => {
  openModal({
    title: <Title order={3}>Consumer settings</Title>,
    children: <ModalBody {...props} />,
    closeOnClickOutside: false,
  });
};

const ModalBody = ({ clusterId, topicName }: ConsumerModalProps) => {
  const { closeAll } = useModals();
  const nowUTC = dayjs.utc().toDate();
  const zeroUTC = dayjs().set("h", 0).set("m", 0).set("s", 0).toDate();
  const form = useForm<ConsumerForm>({
    initialValues: {
      from: "End",
      dateInterval: [nowUTC, nowUTC],
      onlyBeginning: false,
      timeInterval: [zeroUTC, zeroUTC],
      dateFrom: nowUTC,
      timeFrom: zeroUTC,
    },
    validate: {}, //todo
  });

  const getConsumerSettingFrom = (f: ConsumerForm): ConsumerSettingsFrom => {
    if (f.from == "Beginning") return "Beginning";
    else if (f.from == "End") return "End";
    else {
      let stop_timestamp: number | undefined = undefined;
      let start_timestamp = 0;
      if (f.onlyBeginning) {
        const { dateFrom, timeFrom } = f;
        start_timestamp = dateTimeToUnixTimeMs(dateFrom, timeFrom);
      } else {
        const [dateFrom, dateTo] = f.dateInterval;
        const [timeFrom, timeTo] = f.timeInterval;
        start_timestamp = dateTimeToUnixTimeMs(dateFrom, timeFrom);
        stop_timestamp = dateTimeToUnixTimeMs(dateTo, timeTo);
      }
      return {
        Custom: {
          start_timestamp,
          stop_timestamp,
        },
      };
    }
  };
  const onSubmit = async (f: ConsumerForm) => {
    await startConsumer(clusterId, topicName, getConsumerSettingFrom(f));
    closeAll();
  };

  return (
    <form onSubmit={form.onSubmit(onSubmit)}>
      <Stack>
        <Stack spacing={0}>
          <Text weight={"normal"} size={15}>
            Topic
          </Text>
          <Text color="red" weight={"bold"} component="span">
            {topicName}
          </Text>
        </Stack>
        <Divider />
        <Title size={15}>Start consuming from</Title>
        <Chip.Group position="left" multiple={false} {...form.getInputProps("from")}>
          <Chip value="End">End</Chip>
          <Chip value="Beginning">Beginning</Chip>
          <Chip value="Custom">Custom Time</Chip>
        </Chip.Group>
        <Stack hidden={form.values.from != "Custom"}>
          <Checkbox label="Define only beginning" {...form.getInputProps("onlyBeginning", { type: "checkbox" })} />
          <Stack hidden={form.values.onlyBeginning}>
            <DateRangePicker allowSingleDateInRange label="Date interval" {...form.getInputProps("dateInterval")} />
            <TimeRangeInput
              withSeconds
              label="Time interval UTC (from time - to time)"
              {...form.getInputProps("timeInterval")}
            />
          </Stack>
          <Stack hidden={!form.values.onlyBeginning}>
            <DatePicker allowSingleDateInRange label="From date" {...form.getInputProps("dateFrom")} />
            <TimeInput withSeconds label="From time (UTC)" {...form.getInputProps("timeFrom")} />
          </Stack>
        </Stack>
        <Group mt={10} position="right">
          <Button type="submit">Start 🚀</Button>
        </Group>
      </Stack>
    </form>
  );
};

export const dateTimeToUnixTimeMs = (dateUTC: Date, timeUTC: Date): number => {
  // convert to UTC
  timeUTC = dayjs(timeUTC).utc().toDate();
  dateUTC = dayjs(dateUTC).utc().toDate();
  const dateTime = dateUTC.toISOString().substring(0, 10) + timeUTC.toISOString().substring(10);
  return dayjs(new Date(dateTime)).unix() * 1000;
};

type ConsumerForm = {
  from: "Beginning" | "End" | "Custom";
  dateInterval: [Date, Date];
  timeInterval: [Date, Date];
  onlyBeginning: boolean;
  dateFrom: Date;
  timeFrom: Date;
};
